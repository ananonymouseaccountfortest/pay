use serde::Deserialize;
use shrinkwraprs::Shrinkwrap;
use std::convert::{TryFrom, TryInto};
use thiserror::Error;

// TODO: wrap in newtypes
pub type ClientID = u16;
pub type TransactionID = u32;
pub type AmountFloat = f32; // TODO: bleh, convert to integer and pretend this was never a float :D

#[derive(Error, Debug)]
pub enum DeserializationError {
    #[error("missing amount value")]
    MissingAmount,
    #[error("superfluous amount value")]
    SuperfluousAmount,
    #[error("invalid type value: {0}")]
    InvalidType(String),
}

#[derive(Debug, Default, Copy, Clone, Shrinkwrap, PartialOrd, Ord, Eq, PartialEq)]
#[shrinkwrap(mutable)]
pub struct Amount(pub u64);

// TODO: bad name
const AMOUNT_DECIMALS: u32 = 1_0000;

impl TryFrom<AmountFloat> for Amount {
    type Error = DeserializationError;
    fn try_from(amount: AmountFloat) -> Result<Self, Self::Error> {
        // TODO: add sanity checks: too large values, precision loss, negative values
        let amount = (amount * AMOUNT_DECIMALS as f32) as u64;

        Ok(Amount(amount))
    }
}

impl TryFrom<Option<AmountFloat>> for Amount {
    type Error = DeserializationError;
    fn try_from(amount: Option<AmountFloat>) -> Result<Self, Self::Error> {
        match amount {
            None => Err(DeserializationError::MissingAmount),
            Some(v) => v.try_into(),
        }
    }
}

// I wanted to go with straight to internally tagged enum
// with `#[serde(tag = "type")]` but that will not fly with CVS,
// it seems, and I don't have time to dig into it.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub struct Raw {
    r#type: String,
    client: ClientID,
    tx: TransactionID,
    amount: Option<AmountFloat>,
}

pub struct DepositDetails {
    pub client: ClientID,
    pub tx: TransactionID,
    pub amount: Amount,
}

impl TryFrom<Raw> for DepositDetails {
    type Error = DeserializationError;
    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        Ok(DepositDetails {
            client: raw.client,
            tx: raw.tx,
            amount: raw.amount.try_into()?,
        })
    }
}
pub struct DisputeDetails {
    pub client: ClientID,
    pub tx: TransactionID,
}

impl TryFrom<Raw> for DisputeDetails {
    type Error = DeserializationError;
    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        if raw.amount.is_some() {
            return Err(DeserializationError::SuperfluousAmount);
        }

        Ok(DisputeDetails {
            client: raw.client,
            tx: raw.tx,
        })
    }
}

pub type Deposit = DepositDetails;
pub type Withrawal = DepositDetails;
pub type Dispute = DisputeDetails;
pub type Resolve = DisputeDetails;
pub type Chargeback = DisputeDetails;

pub enum Payment {
    Deposit(Deposit),
    Withrawal(Withrawal),
    Dispute(Dispute),
    Resolve(Resolve),
    Chargeback(Chargeback),
}

impl Payment {
    /// Get client id
    ///
    /// Since all payment types have it, it's useful to
    /// have it.
    pub fn get_client_id(&self) -> ClientID {
        match self {
            Payment::Deposit(d) => d.client,
            Payment::Withrawal(d) => d.client,
            Payment::Dispute(d) => d.client,
            Payment::Resolve(d) => d.client,
            Payment::Chargeback(d) => d.client,
        }
    }
}

impl TryFrom<Raw> for Payment {
    type Error = DeserializationError;
    fn try_from(raw: Raw) -> Result<Payment, Self::Error> {
        Ok(match raw.r#type.as_str() {
            "deposit" => Payment::Deposit(raw.try_into()?),
            "withrawal" => Payment::Withrawal(raw.try_into()?),
            "dispute" => Payment::Dispute(raw.try_into()?),
            "resolve" => Payment::Resolve(raw.try_into()?),
            "chargeback" => Payment::Chargeback(raw.try_into()?),
            _ => return Err(DeserializationError::InvalidType(raw.r#type)),
        })
    }
}

#[test]
fn test_payment_deserialization() -> anyhow::Result<()> {
    let input = r#"type,client,tx,amount
deposit,1,1,1.0
withrawal,1,1,1.0
dispute,1,1,
resolve,1,1,
chargeback,1,1,
"#;

    let mut reader = csv::Reader::from_reader(input.as_bytes());
    for payment in reader.deserialize() {
        let payment: Raw = payment?;
        println!("{:?}", payment);
        let _payment: Payment = payment.try_into()?;
    }
    Ok(())
}
