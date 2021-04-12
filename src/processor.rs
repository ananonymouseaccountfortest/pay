use crate::payment::{Amount, ClientID, Payment, TransactionID, Deposit,  Withrawal};
use fnv::FnvHashMap;
use thiserror::Error;

type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
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
}

/// Payment processor
pub trait Processor {
    /// Process a payment
    fn process(&mut self, payment: Payment) -> Result<()>;
}

#[derive(Debug, Clone)]
pub enum PastTransaction {
    Deposit(Amount),
    Withrawal(Amount),
}

#[derive(Debug, Default, Clone)]
pub struct AccountState {
    locked: bool,
    // not storing `available_funds` since it's a straight `total - held` right now
    // but it could be added later if necessary (more cases than just held)
    total_funds: Amount,
    held_funds: Amount,
}

impl AccountState {
    fn available_funds(&self) -> Amount {
        Amount(*self.total_funds - *self.held_funds)
    }

    fn deposit(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();
        *new.total_funds += self
            .total_funds
            .checked_add(*amount)
            .ok_or_else(|| Error::Overflow )?;

        new.ensure_valid_state()
    }

    fn withdraw(&self, amount: Amount) -> Result<Self> {
        let mut new = self.clone();
        *new.total_funds -= self
            .total_funds
            .checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        // can't withraw funds that are not available
        new.available_funds().checked_sub(*amount)
            .ok_or_else(|| Error::Underflow)?;

        new.ensure_valid_state()
    }

    // TODO: this is more of a debugging assertion; remove?
    fn ensure_valid_state(self) -> Result<Self> {
        if *self.total_funds < *self.held_funds {
            return Err(Error::InvalidAccountState );
        }

        Ok(self)
    }
}

impl Account {
    fn deposit(&mut self, details: Deposit) -> Result<()> {
        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists );
        }
        let new_state = self.state.deposit(details.amount)?;
        self.state = new_state;
        self.history.insert(details.tx, PastTransaction::Deposit(details.amount));
        Ok(())
    }

    fn withdraw(&mut self, details: Withrawal) -> Result<()> {
        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists );
        }
        let new_state = self.state.withdraw(details.amount)?;
        self.state = new_state;
        self.history.insert(details.tx, PastTransaction::Withrawal(details.amount));
        Ok(())
    }

    fn dispute(&mut self, details: Withrawal) -> Result<()> {

        let past_tx = match self.history.get(&details.tx).ok_or_else(|| Error::TransactionNotFound)? {
            PastTransaction::Deposit(details) => details,
            // seems like disputing withrawals is not supported?
            PastTransaction::Withrawal(_) => return Err(Error::WrongTransactionType),
        };


        if self.history.contains_key(&details.tx) {
            return Err(Error::TransactionAlreadyExists );
        }
        let new_state = self.state.withdraw(details.amount)?;
        self.state = new_state;
        self.history.insert(details.tx, PastTransaction::Withrawal(details.amount));
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct Account {
    state: AccountState,
    history: FnvHashMap<TransactionID, PastTransaction>,
}

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
            _ => todo!(),
        }
        Ok(())
    }
}
