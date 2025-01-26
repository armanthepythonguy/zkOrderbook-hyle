use clap::{Subcommand, Parser};
use client_sdk::helpers::risc0::Risc0Prover;
use contract_orderbook_app::{OrderBookState, OrderBookAction};
use methods::{ZK_ORDERBOOK_ELF, ZK_ORDERBOOK_ID};
use reqwest::Identity;
use sdk::{ContractName, RegisterContractTransaction, Digestable};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli{

    #[clap(subcommand)]
    pub cmd: Commands,

    #[arg(long, default_value = "http://localhost:4321")]
    pub host: String,

    #[arg(long, default_value = "orderbook_app")]
    pub contract_name: String,
    
    #[arg(long, default_value = "examples.orderbook_app")]
    pub user: String,

    #[arg(long, default_value = "pass")]
    pub pass: String,

    #[arg(long, default_value = "0")]
    pub nonce: String,

}


#[derive(Subcommand)]
enum Commands {
    Register { token: String },
    DepositAsset { token:String, amount: u128 },
    InsertOrder { token: String, price: u64, amount: u64, side: String },
}

#[tokio::main]
async fn main() {

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    let client = client_sdk::rest_client::NodeApiHttpClient::new(cli.host).unwrap();

    let contract_name = &cli.contract_name.clone();

    let orderbook_prover = Risc0Prover::new(ZK_ORDERBOOK_ELF);
    let identity_prover = Risc0Prover::new(methods_identity::GUEST_ELF);
    let token_prover = Risc0Prover::new(methods_token::GUEST_ELF);

    match cli.cmd{

        Commands::Register { token } => {

            let initial_state = OrderBookState::new(ContractName(token));

            let register_tx = RegisterContractTransaction {
                owner: "examples".to_string(),
                verifier: "risc0".into(),
                program_id: sdk::ProgramId(sdk::to_u8_array(&ZK_ORDERBOOK_ID).to_vec()),
                state_digest: initial_state.as_digest(),
                contract_name: contract_name.clone().into(),
            };
            let res = client
                .send_tx_register_contract(&register_tx)
                .await
                .unwrap();

            println!("✅ Register contract tx sent. Tx hash: {}", res);


        },

        Commands::DepositAsset { token, amount } => {

            let initial_state: OrderBookState = client
                .get_contract(&contract_name.clone().into())
                .await
                .unwrap()
                .state
                .into();
            println!("Initial state: {:?}", initial_state);

            let identity = Identity(cli.user.clone());

            let identity

        }

        Commands::InsertOrder { token, price, amount, side } => {}

    }


}