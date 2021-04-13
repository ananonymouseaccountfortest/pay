use serde::{Deserialize, Serialize};
use shrinkwraprs::Shrinkwrap;
use std::convert::{TryFrom, TryInto};
use thiserror::Error;

// TODO: wrap in newtypes?
pub type ClientID = u16;
pub type TransactionID = u32;

#[derive(Error, Debug)]
pub enum DeserializationError {
    #[error("missing amount value")]
    MissingAmount,
    #[error("superfluous amount value")]
    SuperfluousAmount,
    #[error("invalid type value: {0}")]
    InvalidType(String),
}

// TODO: I don't like this type as is right now
// with some boilerplate it could be made into something
// better: checking overflow/underflow, verifying precision
#[derive(Debug, Default, Copy, Clone, Shrinkwrap, PartialOrd, Ord, Eq, PartialEq)]
#[shrinkwrap(mutable)]
pub struct Amount(pub u64);

// TODO: bad name
const AMOUNT_PRECISION: f32 = 0.0001;

impl Amount {
    pub fn to_f32(self) -> f32 {
        self.0 as f32 * AMOUNT_PRECISION
    }
}
impl TryFrom<f32> for Amount {
    type Error = DeserializationError;
    fn try_from(amount: f32) -> Result<Self, Self::Error> {
        // TODO: add sanity checks: too large values, precision loss, negative values
        let amount = (amount / AMOUNT_PRECISION) as u64;

        Ok(Amount(amount))
    }
}

impl TryFrom<Option<f32>> for Amount {
    type Error = DeserializationError;
    fn try_from(amount: Option<f32>) -> Result<Self, Self::Error> {
        match amount {
            None => Err(DeserializationError::MissingAmount),
            Some(v) => v.try_into(),
        }
    }
}

// I wanted to go with straight to internally tagged enum
// with `#[serde(tag = "type")]` but that will not fly with CSV,
// it seems, and I don't have time to dig into it.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub struct RawInputRecord {
    pub r#type: String,
    pub client: ClientID,
    pub tx: TransactionID,
    pub amount: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct RawOutputRecord {
    pub client: ClientID,
    pub available: f32,
    pub held: f32,
    pub total: f32,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct DepositDetails {
    pub client: ClientID,
    pub tx: TransactionID,
    pub amount: Amount,
}

impl TryFrom<RawInputRecord> for DepositDetails {
    type Error = DeserializationError;
    fn try_from(raw: RawInputRecord) -> Result<Self, Self::Error> {
        Ok(DepositDetails {
            client: raw.client,
            tx: raw.tx,
            amount: raw.amount.try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DisputeDetails {
    pub client: ClientID,
    pub tx: TransactionID,
}

impl TryFrom<RawInputRecord> for DisputeDetails {
    type Error = DeserializationError;
    fn try_from(raw: RawInputRecord) -> Result<Self, Self::Error> {
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

#[derive(Clone, Debug)]
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

impl TryFrom<RawInputRecord> for Payment {
    type Error = DeserializationError;
    fn try_from(raw: RawInputRecord) -> Result<Payment, Self::Error> {
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

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(input.as_bytes());
    for payment in reader.deserialize() {
        let payment: RawInputRecord = payment?;
        println!("{:?}", payment);
        let _payment: Payment = payment.try_into()?;
    }
    Ok(())
}
