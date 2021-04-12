use std::convert::TryInto;
use std::io::Read;
use structopt::StructOpt;

mod opts;
mod payment;

fn main() -> anyhow::Result<()> {
    let opts = opts::Opts::from_args();

    let mut reader = csv::Reader::from_reader(std::fs::File::open(opts.input_cvs)?);
    for payment in reader.deserialize() {
        let payment: payment::Raw = payment?;
        let payment: payment::Payment = payment.try_into()?;
    }

    Ok(())
}
