use processor::Processor;
use std::convert::TryInto;
use structopt::StructOpt;

mod opts;
mod payment;
mod processor;

fn run() -> anyhow::Result<()> {
    let opts = opts::Opts::from_args();

    let mut processor = processor::InMemoryProcessor::default();

    // Note: Note that the CSV reader is buffered automatically,
    // so no need for `BufReader`.
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(std::fs::File::open(opts.input_cvs)?);

    for (i, payment) in reader.deserialize().enumerate() {
        let payment_raw: payment::RawInputRecord = payment?;
        let payment: payment::Payment = payment_raw.clone().try_into()?;
        if let Err(e) = processor.process(payment) {
            // just report any errors - even ones that were explicitily listed
            // as conditions we should tolerate;
            // TODO: it remains unclear if we should
            // ever have any conditions that should fail the whole execution
            eprintln!("Error while processing record {} {:?}: {}", i, payment_raw, e);
        }
    }

    let mut writer = csv::Writer::from_writer(std::io::stdout());
    for (client_id, account) in processor.get_all_accounts() {
        writer.serialize(payment::RawOutputRecord {
            client: *client_id,
            available: account.available_funds().to_f32(),
            held: account.held_funds.to_f32(),
            total: account.total_funds.to_f32(),
            locked: account.locked,
        })?;
    }
    writer.flush()?;

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        println!("terminated due to error: {}", err);
        std::process::exit(1);
    }
}
