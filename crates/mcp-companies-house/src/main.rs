use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    mcp_companies_house::server().main().await
}
