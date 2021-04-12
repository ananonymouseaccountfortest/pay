use crate::payment::{
    Amount, Chargeback, ClientID, Deposit, Dispute, Payment, Resolve, TransactionID, Withrawal,
};
use fnv::FnvHashMap;
use thiserror::Error;

type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("invalid account state")]
    InvalidAccountState,
    #[error("balance overflow in account")]
    Overflow,
    #[error("balance underflow in account")]
    Underflow,
    #[error("transaction already existst")]
    TransactionAlreadyExists,
    #[error("transaction not found")]
    TransactionNotFound,
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
    Withrawal(Amount),
}

// State of the account
//
// Operations on it are immutable, so it's
// more natural to attempt a given operation
// and only if it was successful, mutate
// state and other parts of `Account`
#[derive(Debug, Default, Clone)]
pub struct AccountState {
    // TODO: it remains unclear to me what exactly should be dissallowed after
    // account has been locked
    locked: bool,
    // not storing `available_funds` since it's a straight `total - held` right now
    // but it could be added later if necessary (more cases than just held)
    total_funds: Amount,
    held_funds: Amount,
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

        new.ensure_valid_state()
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

        new.ensure_valid_state()
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

        new.ensure_valid_state()
    }

    // TODO: is unhold a really bad name?
    #[must_use]
    fn unhold(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();

        *new.held_funds = new
            .held_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        new.ensure_valid_state()
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

        new.ensure_valid_state()
    }

    // TODO: this is more of a debugging assertion; remove?
    fn ensure_valid_state(self) -> Result<Self> {
        if *self.total_funds < *self.held_funds {
            return Err(Error::InvalidAccountState);
        }

        Ok(self)
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
                PastTransaction::Withrawal(_) => return Err(Error::WrongTransactionType),
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

    fn withdraw(&mut self, details: Withrawal) -> Result<()> {
        if self.state.locked {
            return Err(Error::AccountLocked);
        }

        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists);
        }
        self.state = self.state.withdraw(details.amount)?;
        self.history
            .insert(details.tx, PastTransaction::Withrawal(details.amount));
        Ok(())
    }

    fn dispute(&mut self, details: Dispute) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;

        self.state = self.state.hold(past_tx)?;
        Ok(())
    }

    fn resolve(&mut self, details: Resolve) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;

        self.state = self.state.unhold(past_tx)?;
        Ok(())
    }

    fn chargeback(&mut self, details: Chargeback) -> Result<()> {
        let past_tx = self.get_past_deposit(details.tx)?;

        self.state = self.state.chargeback(past_tx)?;
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
struct Account {
    state: AccountState,
    history: FnvHashMap<TransactionID, PastTransaction>,
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
            Payment::Withrawal(details) => {
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

    processor.process(Payment::Withrawal(Withrawal {
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
        .process(Payment::Withrawal(Withrawal {
            client,
            tx: 5,
            amount: Amount(2),
        }))
        .unwrap();

    processor
        .process(Payment::Withrawal(Withrawal {
            client,
            tx: 6,
            amount: Amount(2),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 1);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 1);

    Ok(())
}

#[test]
fn funds_on_hold_math_checks_out() -> Result<()> {
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
        .process(Payment::Withrawal(Withrawal {
            client,
            tx: 12,
            amount: Amount(1),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);

    // can't resolve wrong tx
    assert_eq!(
        processor.process(Payment::Resolve(Resolve { client, tx: 5 })),
        Err(Error::TransactionNotFound)
    );

    // resolve dispute now
    processor.process(Payment::Resolve(Resolve { client, tx: 3 }))?;

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 7);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 7);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);

    processor
        .process(Payment::Withrawal(Withrawal {
            client,
            tx: 13,
            amount: Amount(7),
        }))
        .unwrap();

    assert_eq!(*processor.get_account(client).unwrap().total_funds, 0);
    assert_eq!(*processor.get_account(client).unwrap().available_funds(), 0);
    assert_eq!(*processor.get_account(client).unwrap().held_funds, 0);

    Ok(())
}

#[test]
fn withdrawal_underflow() -> Result<()> {
    let mut processor = InMemoryProcessor::default();

    processor
        .process(Payment::Deposit(Deposit {
            client: 3,
            tx: 3,
            amount: Amount(1),
        }))
        .unwrap();

    assert_eq!(
        processor.process(Payment::Withrawal(Withrawal {
            client: 3,
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

    assert_eq!(
        processor.process(Payment::Dispute(Dispute { client: 3, tx: 4 })),
        Err(Error::TransactionNotFound)
    );

    Ok(())
}

#[test]
fn chargeback_unknown_tx() -> Result<()> {
    let mut processor = InMemoryProcessor::default();

    assert_eq!(
        processor.process(Payment::Chargeback(Dispute { client: 3, tx: 4 })),
        Err(Error::TransactionNotFound)
    );

    Ok(())
}
