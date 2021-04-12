use processor::Processor;
use std::convert::TryInto;
use structopt::StructOpt;

mod opts;
mod payment;
mod processor;

fn main() -> anyhow::Result<()> {
    let opts = opts::Opts::from_args();

    let mut processor = processor::InMemoryProcessor::default();

    // TODO: check if csv needs `BufReader`; important for perf.
    let mut reader = csv::Reader::from_reader(std::fs::File::open(opts.input_cvs)?);
    for payment in reader.deserialize() {
        let payment: payment::Raw = payment?;
        let payment: payment::Payment = payment.try_into()?;
        processor.process(payment)?;
    }

    Ok(())
}
