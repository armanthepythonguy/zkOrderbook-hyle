use std::collections::HashMap;

use bincode::{Decode, Encode};
use sdk::{erc20::{self, ERC20Action}, BlobIndex, ContractInput, ContractName, Digestable, HyleOutput, Identity, RunResult};
use serde::{Deserialize, Serialize};

pub fn execute(contract_input: ContractInput) -> HyleOutput{

    let (input, orderbook_action) = sdk::guest::init_raw::<OrderBookAction>(contract_input);
    let orderbook_contract_name = input.blobs.get(input.index.0).unwrap().contract_name.clone();

    let orderbook_state: OrderBookState = input.initial_state.clone().into();

    let mut orderbook_contract = OrderBookContract::new(
        input.identity.clone(),
        orderbook_contract_name,
        orderbook_state,
    );

    let res = match orderbook_action{

        OrderBookAction::DepositAsset{} => {
            let transfer_action =
            sdk::utils::parse_blob::<ERC20Action>(input.blobs.as_slice(), &BlobIndex(1));
    
            let transfer_action_contract_name = input.blobs.get(1).unwrap().contract_name.clone();
    
            orderbook_contract.deposit_asset(transfer_action, transfer_action_contract_name)
        }

        OrderBookAction::InsertOrder { order_asset, order_type, order_price, order_quantity } => {
            let order = Order { order_actor: input.identity.clone(), order_type: order_type, order_price: order_price, order_quantity: order_quantity };
            orderbook_contract.insert_order(order, ContractName(order_asset))
        }

    };

    sdk::utils::as_hyle_output(input, orderbook_contract.state, res)

}

#[derive(Encode, Decode, Debug, Clone)]
pub enum OrderBookAction {
    DepositAsset{},
    InsertOrder{order_asset: String, order_type: OrderType, order_price: f64, order_quantity: u128},
}

#[derive(Encode, Decode, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderType{
    Ask,
    Bid
}


#[derive(Encode, Decode, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Order{
    pub order_actor: Identity,
    pub order_type: OrderType,
    pub order_price: f64,
    pub order_quantity: u128,
}

#[derive(Encode, Decode, Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub ask_orders: Vec<Order>,
    pub bid_orders: Vec<Order>,
}

#[derive(Encode, Decode, Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookState{
    pub base_asset: String,
    pub markets: HashMap<String, Market>,
    pub balances: HashMap<String, HashMap<String, u128>>,
}

pub struct OrderBookContract{
    identity: Identity,
    contract_name: ContractName,
    pub state: OrderBookState,
}

impl OrderBookState{
    pub fn new(base: String) -> Self{
        OrderBookState{
            markets: HashMap::new(),
            balances: HashMap::new(),
            base_asset: base,
        }
    }
}

impl Market{

    pub fn reorder_ask(&mut self){
        self.ask_orders.sort_by(|a, b| a.order_price.partial_cmp(&b.order_price).unwrap());
    }   

    pub fn reorder_bid(&mut self){
        self.bid_orders.sort_by(|a, b| b.order_price.partial_cmp(&a.order_price).unwrap());
    }

}



impl OrderBookContract{
    
    pub fn new(identity: Identity, contract_name: ContractName, state: OrderBookState) -> Self{
        OrderBookContract{
            identity,
            contract_name,
            state: state,
        }
    }

    pub fn deposit_asset(&mut self, erc20_action : erc20::ERC20Action, erc20_name: ContractName) -> RunResult{

        let mut balance = 0;

        match erc20_action{

            erc20::ERC20Action::Transfer { recipient, amount } => {

                if recipient != self.contract_name.0{
                    return Err(format!(
                        "Transfer recipient should be {} but was {}",
                        self.contract_name.0, &recipient
                    ));
                }

                balance = amount;

            }

            els => {
                return Err(format!(
                    "Wrong ERC20Action"
                ));
            }

        }

        let program_outputs = format!("Deposit success for {:?} - {:?}", self.identity.clone(), erc20_name.clone());

        self.state.balances.entry(self.identity.0.clone()).or_insert(HashMap::new()).entry(erc20_name.0.clone()).and_modify(|e| *e += balance).or_insert(balance);

        Ok(program_outputs)

    }

    pub fn insert_order(&mut self, order: Order, market_name: ContractName) -> RunResult{

        let market = self.state.markets.entry(market_name.0.clone()).or_insert(Market{ask_orders: Vec::new(), bid_orders: Vec::new()});

        match order.order_type{
            OrderType::Bid => {

                if(self.state.balances.get(&self.identity.0.clone()).unwrap().get(&self.state.base_asset).unwrap() < &(order.order_price.round() as u128 * order.order_quantity)){
                    return Err(format!(
                        "Insufficient balance for {:?} - {:?}", self.identity.clone(), market_name.clone()
                    ));
                }else{
                    let amount = order.order_price.round() as u128 * order.order_quantity;
                    self.state.balances.entry(self.identity.0.clone()).or_insert(HashMap::new()).entry(self.state.base_asset.clone()).and_modify(|e| *e -= amount).or_insert(0);
                }

                match process_order(&mut order.clone(), market){
                    Some((bid_actor, ask_actor, matched_quantity, matched_price)) => {
                        let matched_amount = matched_quantity as f64 * matched_price;
                        self.state.balances.entry(bid_actor.0.clone()).or_insert(HashMap::new()).entry(self.state.base_asset.clone()).and_modify(|e| *e += matched_amount as u128).or_insert(0);
                        self.state.balances.entry(ask_actor.0.clone()).or_insert(HashMap::new()).entry(market_name.0.clone()).and_modify(|e| *e += matched_quantity).or_insert(0);
                    },
                    None => {}
                };
                
            }
            OrderType::Ask => {


                if(self.state.balances.get(&self.identity.0.clone()).unwrap().get(&market_name.0.clone()).unwrap() < &order.order_quantity){
                    return Err(format!(
                        "Insufficient balance for {:?} - {:?}", self.identity.clone(), market_name.clone()
                    ));
                }else{
                    self.state.balances.entry(self.identity.0.clone()).or_insert(HashMap::new()).entry(market_name.0.clone()).and_modify(|e| *e -= order.order_quantity).or_insert(0);
                }

                match process_order(&mut order.clone(), market){
                    Some((bid_actor, ask_actor, matched_quantity, matched_price)) => {
                        let matched_amount = matched_quantity as f64 * matched_price;
                        self.state.balances.entry(bid_actor.0.clone()).or_insert(HashMap::new()).entry(self.state.base_asset.clone()).and_modify(|e| *e += matched_amount as u128).or_insert(0);
                        self.state.balances.entry(ask_actor.0.clone()).or_insert(HashMap::new()).entry(market_name.0.clone()).and_modify(|e| *e += matched_quantity).or_insert(0);
                    },
                    None => {}
                };
                
            }
        }

        let program_outputs = format!("Order inserted successfully for {:?} - {:?}", self.identity.clone(), market_name.clone());

        Ok(program_outputs)

    }

}

fn process_order(order: &mut Order, market: &mut Market) -> Option<(Identity, Identity, u128, f64)> {
    match order.order_type{
        OrderType::Ask =>{

            // Finding if any order matches the current order
            let mut bid_orders = market.clone().bid_orders;
            let matched_order_index = bid_orders.iter_mut().position(|x| x.order_price >= order.order_price);
            match matched_order_index{
                // If any order has matched
                Some(matched_index) => {
                    let mut matched_order = market.bid_orders[matched_index].clone();
                    let matched_quantity = std::cmp::min(order.order_quantity, matched_order.order_quantity);
                    
                    // Checking if the matched order has more quantity
                    if matched_order.order_quantity > matched_quantity{
                        matched_order.order_quantity -= matched_quantity;
                        market.bid_orders[matched_index] = matched_order.clone();
                    } else {
                        market.bid_orders.remove(matched_index);
                    }

                    // Checking if the new order has more quantity
                    if order.order_quantity > matched_quantity{
                        order.order_quantity -= matched_quantity;
                        market.ask_orders.push(order.clone());
                        market.reorder_ask();
                    }

                    // Returning bid_actor, ask_actor, matched_quantity, matched_price
                    Some((matched_order.order_actor.clone(), order.order_actor.clone(), matched_quantity, matched_order.order_price))
                }
                // If no order has matched
                None => {
                    market.ask_orders.push(order.clone());
                    market.reorder_ask();
                    None
                }
            }
        }

        OrderType::Bid => {
        
            // Finding if any order matches the current order
            let mut ask_orders = market.clone().ask_orders;
            let matched_order_index = ask_orders.iter_mut().position(|x| x.order_price <= order.order_price);
            match matched_order_index{

                Some(matched_index) => {
                    let mut matched_order = market.ask_orders[matched_index].clone();
                    let matched_quantity = std::cmp::min(order.order_quantity, matched_order.order_quantity);
                    
                    // Checking if the matched order has more quantity
                    if matched_order.order_quantity > matched_quantity{
                        matched_order.order_quantity -= matched_quantity;
                        market.ask_orders[matched_index] = matched_order.clone();
                    }else{
                        market.ask_orders.remove(matched_index);
                    }

                    // Checking if the new order has more quantity
                    if order.order_quantity > matched_quantity{
                        order.order_quantity -= matched_quantity;
                        market.bid_orders.push(order.clone());
                        market.reorder_bid();
                    }

                    // Returning bid_actor, ask_actor, matched_quantity, matched_price
                    Some((order.order_actor.clone(), matched_order.order_actor.clone(), matched_quantity, matched_order.order_price))
                }

                None => {
                    market.bid_orders.push(order.clone());
                    market.reorder_bid();
                    None
                }

            }

        }
    }

}

impl Digestable for OrderBookState{

    fn as_digest(&self) -> sdk::StateDigest {
        sdk::StateDigest(
            bincode::encode_to_vec(self, bincode::config::standard())
                .expect("Failed to encode TicketAppState"),
        )
    }

}

impl From<sdk::StateDigest> for OrderBookState {
    fn from(state: sdk::StateDigest) -> Self {
        let (orderbook_state, _) =
            bincode::decode_from_slice(&state.0, bincode::config::standard())
                .expect("Could not decode OrderbookState");
        orderbook_state
    }
}