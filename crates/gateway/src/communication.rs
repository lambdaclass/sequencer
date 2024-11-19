use starknet_mempool_infra::component_server::{EmptyServer, create_empty_server};

use crate::gateway::Gateway;

pub type GatewayServer = EmptyServer<Gateway>;

pub fn create_gateway_server(gateway: Gateway) -> GatewayServer {
    create_empty_server(gateway)
}
