use crate::payment::{
    Amount, Chargeback, ClientID, Deposit, Dispute, Payment, Resolve, TransactionID, Withdrawal,
};
use fnv::{FnvHashMap, FnvHashSet};
use thiserror::Error;

type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("balance overflow in account")]
    Overflow,
    #[error("balance underflow in account")]
    Underflow,
    #[error("transaction already existst")]
    TransactionAlreadyExists,
    #[error("transaction not found")]
    TransactionNotFound,
    #[error("transaction not under dispute")]
    TransactionNotDisputed,
    #[error("transaction already under dispute")]
    TransactionAlreadyDisputed,
    #[error("wrong transaction type")]
    WrongTransactionType,
    #[error("account locked")]
    AccountLocked,
}

/// Payment processor
///
/// The API that an implementation of a payment processor provides
pub trait Processor {
    /// Process a payment
    fn process(&mut self, payment: Payment) -> Result<()>;
    fn get_all_accounts(&self) -> Box<dyn Iterator<Item = (&ClientID, &AccountState)> + '_>;
    fn get_all_clients(&self) -> Box<dyn Iterator<Item = &ClientID> + '_>;
    fn get_account(&self, client_id: ClientID) -> Option<&AccountState>;
}

#[derive(Debug, Clone)]
pub enum PastTransaction {
    Deposit(Amount),
    Withdrawal(Amount),
}

// State of the account
//
// Operations on it are immutable, so it's
// more natural to attempt a given operation
// and only if it was successful, mutate
// state and other parts of the `Account`
#[derive(Debug, Default, Clone)]
pub struct AccountState {
    // TODO: it remains unclear to me what exactly should be dissallowed after
    // account has been locked
    pub locked: bool,
    // not storing `available_funds` since it's a straight `total - held` right now
    // but it could be added later if necessary (more cases than just held)
    pub total_funds: Amount,
    pub held_funds: Amount,
}

impl AccountState {
    pub fn available_funds(&self) -> Amount {
        Amount(*self.total_funds - *self.held_funds)
    }

    #[must_use]
    fn deposit(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        *new.total_funds = new
            .total_funds
            .checked_add(*amount)
            .ok_or_else(|| Error::Overflow)?;

        Ok(new)
    }

    #[must_use]
    fn withdraw(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        // can't withraw funds that are not available
        new.available_funds()
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        *new.total_funds = new
            .total_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        Ok(new)
    }

    #[must_use]
    fn hold(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        // can't hold funds that are not available
        new.available_funds()
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        *new.held_funds = new
            .held_funds
            .checked_add(*amount)
            .ok_or_else(|| Error::Overflow)?;

        Ok(new)
    }

    // TODO: is unhold a really bad name?
    #[must_use]
    fn unhold(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        *new.held_funds = new
            .held_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        Ok(new)
    }

    #[must_use]
    fn chargeback(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        *new.total_funds = new
            .total_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        *new.held_funds = new
            .held_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        new.locked = true;

        Ok(new)
    }
}

impl Account {
    fn get_past_deposit(&self, tx: TransactionID) -> Result<Amount> {
        Ok(
            match self
                .history
                .get(&tx)
                .ok_or_else(|| Error::TransactionNotFound)?
            {
                PastTransaction::Deposit(details) => *details,
                // seems like disputing withrawals is not supported?
                PastTransaction::Withdrawal(_) => return Err(Error::WrongTransactionType),
            },
        )
    }

    fn deposit(&mut self, details: Deposit) -> Result<()> {
        if self.state.locked {
            return Err(Error::AccountLocked);
        }

        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists);
        }
        let new_state = self.state.deposit(details.amount)?;
        self.state = new_state;
        self.history
            .insert(details.tx, PastTransaction::Deposit(details.amount));
        Ok(())
    }

    fn withdraw(&mut self, details: Withdrawal) -> Result<()> {
        if self.state.locked {
            return Err(Error::AccountLocked);
        }

        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists);
        }
        self.state = self.state.withdraw(details.amount)?;
        self.history
            .insert(details.tx, PastTransaction::Withdrawal(details.amount));
        Ok(())
    }

    fn dispute(&mut self, details: Dispute) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;
        if self.in_dispute.contains(&details.tx) {
            return Err(Error::TransactionAlreadyDisputed);
        }

        self.state = self.state.hold(past_tx)?;
        self.in_dispute.insert(details.tx);
        Ok(())
    }

    fn resolve(&mut self, details: Resolve) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;
        if !self.in_dispute.contains(&details.tx) {
            return Err(Error::TransactionNotDisputed);
        }

        self.state = self.state.unhold(past_tx)?;
        self.in_dispute.remove(&details.tx);
        Ok(())
    }

    fn chargeback(&mut self, details: Chargeback) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;
        if !self.in_dispute.contains(&details.tx) {
            return Err(Error::TransactionNotDisputed);
        }

        self.state = self.state.chargeback(past_tx)?;
        self.in_dispute.remove(&details.tx);
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
struct Account {
    state: AccountState,
    history: FnvHashMap<TransactionID, PastTransaction>,
    in_dispute: FnvHashSet<TransactionID>,
}

/**
 * Simple processor implementation that keeps track of everything in the memory.
 */
#[derive(Default)]
pub struct InMemoryProcessor {
    accounts: FnvHashMap<ClientID, Account>,
}

impl Processor for InMemoryProcessor {
    fn process(&mut self, payment: Payment) -> Result<()> {
        let account = self.accounts.entry(payment.get_client_id()).or_default();
        match payment {
            Payment::Deposit(details) => {
                account.deposit(details)?;
            }
            Payment::Withdrawal(details) => {
                account.withdraw(details)?;
            }
            Payment::Dispute(details) => {
                account.dispute(details)?;
            }
            Payment::Resolve(details) => {
                account.resolve(details)?;
            }
            Payment::Chargeback(details) => {
                account.chargeback(details)?;
            }
        }
        Ok(())
    }

    fn get_all_accounts(&self) -> Box<dyn Iterator<Item = (&ClientID, &AccountState)> + '_> {
        Box::new(
            self.accounts
                .iter()
                .map(|(id, account)| (id, &account.state)),
        )
    }

    fn get_all_clients(&self) -> Box<dyn Iterator<Item = &ClientID> + '_> {
        Box::new(self.accounts.keys())
    }

    fn get_account(&self, client_id: ClientID) -> Option<&AccountState> {
        self.accounts.get(&client_id).map(|account| &account.state)
    }
}

#[test]
fn basic_happy_case() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 1;

    processor.process(Payment::Deposit(Deposit {
        client,
        tx: 3,
        amount: Amount(1),
    }))?;

    processor.process(Payment::Withdrawal(Withdrawal {
        client,
        tx: 4,
        amount: Amount(1),
    }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 0);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);

    Ok(())
}

#[test]
fn basic_multi_payment_math_checks_out() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    processor
        .process(Payment::Deposit(Deposit {
            client,
            tx: 3,
            amount: Amount(1),
        }))
        .unwrap();

    processor
        .process(Payment::Deposit(Deposit {
            client,
            tx: 4,
            amount: Amount(4),
        }))
        .unwrap();

    processor
        .process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 5,
            amount: Amount(2),
        }))
        .unwrap();

    processor
        .process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 6,
            amount: Amount(2),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 1);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 1);

    Ok(())
}

// Note: there's a lot of stuff being tested here; might be
// better to split it into separate unit-test functions, but that
// would cause even more boilerplate, setting up the state
// for each step, and I'm getting a bit tired with this
// exercise already. In production code I might have
// do the effort, and maybe add some nice setup
// code or even macros to make this easier.
#[test]
fn funds_on_hold_math_and_basic_flow() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    processor.process(Payment::Deposit(Deposit {
        client,
        tx: 3,
        amount: Amount(7),
    }))?;

    processor.process(Payment::Deposit(Deposit {
        client,
        tx: 4,
        amount: Amount(1),
    }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 8);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 8);

    processor.process(Payment::Dispute(Dispute { client, tx: 3 }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 8);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 1);

    // withdraw everything while rest is disputed
    processor
        .process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 12,
            amount: Amount(1),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);

    // can't dispute same tx twice
    assert_eq!(
        processor.process(Payment::Dispute(Dispute { client, tx: 3 })),
        Err(Error::TransactionAlreadyDisputed)
    );

    // can't resolve wrong tx
    assert_eq!(
        processor.process(Payment::Resolve(Resolve { client, tx: 500 })),
        Err(Error::TransactionNotFound)
    );

    // can't resolve tx not under dispute
    assert_eq!(
        processor.process(Payment::Resolve(Resolve { client, tx: 4 })),
        Err(Error::TransactionNotDisputed)
    );

    // resolve dispute now
    processor.process(Payment::Resolve(Resolve { client, tx: 3 }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 7);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);

    // can dispute this tx again (?)
    processor.process(Payment::Dispute(Dispute { client, tx: 3 }))?;
    processor.process(Payment::Resolve(Resolve { client, tx: 3 }))?;

    processor
        .process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 13,
            amount: Amount(7),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 0);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);

    // trying to dispute this tx again would cause a negative balance
    assert_eq!(
        processor.process(Payment::Dispute(Dispute { client, tx: 3 })),
        Err(Error::Underflow)
    );

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 0);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);

    Ok(())
}

#[test]
fn withdrawal_underflow() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    processor
        .process(Payment::Deposit(Deposit {
            client,
            tx: 3,
            amount: Amount(1),
        }))
        .unwrap();

    assert_eq!(
        processor.process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 4,
            amount: Amount(2),
        })),
        Err(Error::Underflow)
    );

    Ok(())
}

#[test]
fn dispute_unknown_tx() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    assert_eq!(
        processor.process(Payment::Dispute(Dispute { client, tx: 4 })),
        Err(Error::TransactionNotFound)
    );

    Ok(())
}

#[test]
fn chargeback_unknown_tx() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    assert_eq!(
        processor.process(Payment::Chargeback(Dispute { client, tx: 4 })),
        Err(Error::TransactionNotFound)
    );

    Ok(())
}

#[test]
fn basic_chargeback_flow() -> Result<()> {
    let mut processor = InMemoryProcessor::default();
    let client = 3;

    processor.process(Payment::Deposit(Deposit {
        client,
        tx: 0,
        amount: Amount(2),
    }))?;

    processor.process(Payment::Deposit(Deposit {
        client,
        tx: 1,
        amount: Amount(1),
    }))?;

    processor.process(Payment::Dispute(Dispute { client, tx: 0 }))?;
    assert_eq!(*processor.get_account(client).unwrap().total_funds, 3);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 1);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 2);
    assert_eq!(processor.get_account(client).unwrap().locked, false);

    assert_eq!(
        processor.process(Payment::Chargeback(Resolve { client, tx: 1 })),
        Err(Error::TransactionNotDisputed)
    );
    assert_eq!(processor.get_account(client).unwrap().locked, false);

    processor.process(Payment::Chargeback(Dispute { client, tx: 0 }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 1);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 1);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);
    assert_eq!(processor.get_account(client).unwrap().locked, true);

    assert_eq!(
        processor.process(Payment::Withdrawal(Withdrawal {
            client,
            tx: 2,
            amount: Amount(1),
        })),
        Err(Error::AccountLocked)
    );
    Ok(())
}
