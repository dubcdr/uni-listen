extern crate core;

use std::time::Duration;

use anyhow::{Ok as AnyhowOk, Result};
use core::result::Result::Ok;
use std::env;
use rayon::prelude::*;
use dotenv::dotenv;

use ethers::contract::AbiError;
use ethers::prelude::*;
use ethers::providers::Http;
use ethers::types::Transaction;
use paris::Logger;
use std::sync::{Arc, Mutex};
use uni_listen::{INFURA_HTTP_ENDPOINT, INFURA_WS_ENDPOINT, UNISWAP_ADDR};

abigen!(
    IUniswapV2Router,
    "./uniswap-v2-abi.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let infura_project_id = env::var("INFURA_PROJECT_ID").expect("Need infura project id");

    let ws = Ws::connect(format!("{}/{}", INFURA_WS_ENDPOINT, infura_project_id)).await?;
    let provider = Provider::new(ws).interval(Duration::from_millis(2000));
    let mut stream = provider.watch_blocks().await?;

    let client = Provider::<Http>::try_from(format!("{}/{}", INFURA_HTTP_ENDPOINT, infura_project_id)).unwrap();
    let arc_client = Arc::new(client.clone());

    let address = UNISWAP_ADDR.parse::<Address>()?;
    let contract = IUniswapV2Router::new(address, arc_client.clone());

    let logger = Logger::new();

    let logger_ref = Arc::new(Mutex::new(logger));

    let logger = Arc::clone(&logger_ref);
    let mut logger = logger.lock().unwrap();
    logger.loading("Waiting for next transaction...");
    drop(logger);

    while let Some(block) = stream.next().await {
        let full_block = client
            .get_block_with_txs(block)
            .await?
            .expect("oh shit, block probably hasnt arrived");

        // filter to uniswap transactions
        let uniswap_txns: Vec<&Transaction> = filter_uni_txns(&full_block);

        let logger = Arc::clone(&logger_ref);
        let mut logger = logger.lock().unwrap();
        logger
            .done()
            .info(format!("New block {}", &full_block.hash.unwrap()));
        if uniswap_txns.len() == 0 {
            logger.info("No uniswap transactions");
        }
        drop(logger);

        // decode and log
        uniswap_txns.par_iter().for_each(|txn| {
            let inputs: Result<(U256, Vec<Address>, Address, U256), AbiError> =
                // swapExactETHForTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline)
                contract.decode("swapExactETHForTokens", &txn.input);

            let txn_message = format!("txn :: {}", &txn.hash());
            match inputs {
                Ok(inputs) => {
                    // let paths: Vec<Address> = inputs.1;
                    // for path in paths {
                    //     logger
                    //         .indent(2)
                    //         .log(format!("through: {}", path.to_string()));
                    // }
                    let logger = Arc::clone(&logger_ref);
                    let mut logger = logger.lock().unwrap();
                    logger
                        .indent(1)
                        .log(txn_message)
                        .indent(2)
                        .log(format!("swap {} ethereum", txn.value))
                        .indent(2)
                        .log(format!("amountOutMin: {}", inputs.0))
                        .indent(2)
                        .log(format!("to: {}", inputs.2));
                }
                Err(_err) => {
                    let logger = Arc::clone(&logger_ref);
                    let mut logger = logger.lock().unwrap();
                    logger
                        .indent(1)
                        .log(txn_message)
                        .indent(2)
                        .log("Unsupported Uniswap Method");
                    // .same()
                    // .log(format!("[{}]", err));
                }
            };
        });
        // for txn in uniswap_txns {
        //     logger.indent(1).log(format!("Txn :: {}", &txn.hash()));
        //     let inputs: Result<(U256, Vec<Address>, Address, U256), AbiError> =
        //         contract.decode("swapExactETHForTokens", &txn.input);
        //     match inputs {
        //         Ok(inputs) => {
        //             // swapExactETHForTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline)
        //             let paths: Vec<Address> = inputs.1;
        //             logger
        //                 .indent(2)
        //                 .log(format!("swap {} ethereum", txn.value))
        //                 .indent(2)
        //                 .log(format!("amountOutMin: {}", inputs.0))
        //                 .indent(2)
        //                 .log(format!("to: {}", inputs.2));
        //             // logger.log(format!("path: ${}", inputs.1));
        //             for path in paths {
        //                 logger
        //                     .indent(2)
        //                     .log(format!("through: {}", path.to_string()));
        //             }
        //         }
        //         Err(err) => {
        //             logger
        //                 .indent(2)
        //                 .log("Unsupported Uniswap Method")
        //                 .same()
        //                 .log(format!("[{}]", err));
        //         }
        //     };
        // }
        let logger = Arc::clone(&logger_ref);
        let mut logger = logger.lock().unwrap();
        logger.loading("Waiting for next transaction...");
    }

    AnyhowOk(())
}

fn filter_uni_txns(full_block: &Block<Transaction>) -> Vec<&Transaction> {
    full_block
        .transactions
        .par_iter()
        .filter(|txn| {
            let is_uniswap_txn: bool = match txn.to {
                Some(to_address) => {
                    let uniswap_addr = UNISWAP_ADDR
                        .parse::<H160>()
                        .expect("Can't parse string to H160");
                    let to_uniswap = to_address == uniswap_addr;
                    to_uniswap
                }
                None => false,
            };
            is_uniswap_txn
        })
        .collect()
}
