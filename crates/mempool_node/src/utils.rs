use crate::communication::{MempoolNodeClients, create_node_channels, create_node_clients};
use crate::components::create_components;
use crate::config::MempoolNodeConfig;
use crate::servers::{Servers, create_servers};

pub fn create_clients_servers_from_config(
    config: &MempoolNodeConfig,
) -> (MempoolNodeClients, Servers) {
    let mut channels = create_node_channels();
    let clients = create_node_clients(config, &mut channels);
    let components = create_components(config, &clients);
    let servers = create_servers(config, &mut channels, components);

    (clients, servers)
}
