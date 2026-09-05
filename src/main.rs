mod cli;
mod client;
mod models;
mod output;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;

#[tokio::main]
async fn main() -> ExitCode {
    // Usage errors exit 2 via clap's own handling, which matches the documented
    // exit code table.
    let cli = Cli::parse();
    let printer = cli.global.printer();

    match run(&cli, &printer).await {
        Ok(()) => ExitCode::from(client::exit::OK),
        Err(err) => {
            printer.emit_error(&err);
            ExitCode::from(err.exit)
        }
    }
}

async fn run(cli: &Cli, printer: &output::Printer) -> client::Result<()> {
    let client = cli.global.client()?;
    let output = cli.command.run(&client).await?;

    if client.is_dry_run() {
        // The command produced no real data; what it would have sent is the
        // whole answer.
        printer.emit_dry_run(&client.take_dry_run_log());
        return Ok(());
    }

    printer.emit(&output);
    Ok(())
}
