use clap::{Subcommand, Parser};
use client_sdk::helpers::risc0::Risc0Prover;
use contract_identity::IdentityContractState;
use contract_orderbook_app::{OrderBookState, OrderBookAction};
use methods::{ZK_ORDERBOOK_ELF, ZK_ORDERBOOK_ID};
use sdk::{identity_provider::IdentityAction, BlobTransaction, ContractInput, ContractName, Digestable, Identity, ProofTransaction, RegisterContractTransaction};
use contract_token::TokenContractState;

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

            let identity_cf: IdentityAction = IdentityAction::VerifyIdentity { account: identity.0.clone(), nonce: cli.nonce.parse().unwrap() };

            let identity_contract_name = cli.user.rsplit_once(".").unwrap().1.to_string();

            let blobs = vec![
                sdk::Blob{
                    contract_name: identity_contract_name.clone().into(),
                    data: sdk::BlobData(
                        bincode::encode_to_vec(identity_cf, bincode::config::standard())
                            .expect("Failed to encode identity action")
                    ),
                },

                sdk::Blob{
                    contract_name: token.clone().into(),
                    data: sdk::BlobData(
                        bincode::encode_to_vec(sdk::erc20::ERC20Action::Transfer { recipient: contract_name.clone(), amount: amount }, bincode::config::standard()).expect("Failed to encode token action")
                    )
                },

                sdk::Blob{
                    contract_name: contract_name.clone().into(),
                    data: sdk::BlobData(bincode::encode_to_vec(contract_orderbook_app::OrderBookAction::DepositAsset {  }, bincode::config::standard()).expect("Failed to encode orderbook action"))
                }
            ];

            let blob_tx = BlobTransaction{
                blobs: blobs.clone(),
                identity: identity.clone()
            };

            let blob_tx = client.send_tx_blob(&blob_tx).await.unwrap();
            println!("✅ Blob tx sent. Tx hash: {}", blob_tx);

            // Proving orderbook tx
            let inputs = ContractInput{
                initial_state: initial_state.as_digest(),
                identity: identity.clone(),
                tx_hash: blob_tx.clone().into(),
                private_blob: sdk::BlobData(vec![]),
                blobs: blobs.clone(),
                index: sdk::BlobIndex(2),
            };
            let proof = orderbook_prover.prove(inputs).await.unwrap();
            let proof_tx = ProofTransaction{
                proof,
                contract_name: contract_name.clone().into(),
            };
            let proof_tx_hash = client.send_tx_proof(&proof_tx).await.unwrap();
            println!("✅ Proof tx sent. Tx hash: {}", proof_tx_hash);

            // Proving token tx
            let initial_token_state: TokenContractState = client.get_contract(&token.clone().into()).await.unwrap().state.into();

            let inputs = ContractInput{
                initial_state: initial_token_state.as_digest(),
                identity: identity.clone(),
                tx_hash: blob_tx.clone().into(),
                private_blob: sdk::BlobData(vec![]),
                blobs: blobs.clone(),
                index: sdk::BlobIndex(1),
            };

            let proof = token_prover.prove(inputs).await.unwrap();

            let proof_tx = ProofTransaction{
                proof,
                contract_name: token.clone().into(),
            };

            let proof_tx_hash = client.send_tx_proof(&proof_tx).await.unwrap();
            println!("✅ Proof tx sent. Tx hash: {}", proof_tx_hash);

            // Proving identity tx
            let initial_identity_state: IdentityContractState = client.get_contract(&identity_contract_name.clone().into()).await.unwrap().state.into();

            let inputs = ContractInput{
                initial_state: initial_identity_state.as_digest(),
                identity: identity.clone(),
                tx_hash: blob_tx.clone().into(),
                private_blob: sdk::BlobData(cli.pass.as_bytes().to_vec()),
                blobs: blobs.clone(),
                index: sdk::BlobIndex(0),
            };

            let proof = identity_prover.prove(inputs).await.unwrap();
            let proof_tx = ProofTransaction{
                proof,
                contract_name: identity_contract_name.clone().into(),
            };

            let proof_tx_hash = client.send_tx_proof(&proof_tx).await.unwrap();
            println!("✅ Proof tx sent. Tx hash: {}", proof_tx_hash);

        }

        Commands::InsertOrder { token, price, amount, side } => {}

    }


}