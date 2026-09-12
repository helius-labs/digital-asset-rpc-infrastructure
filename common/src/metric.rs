use std::net::UdpSocket;

use cadence::{BufferedUdpMetricSink, QueuingMetricSink, StatsdClient};
use cadence_macros::set_global_default;

#[macro_export]
macro_rules! metric {
    {$($block:stmt;)*} => {
        if cadence_macros::is_global_default_set() {
            $(
                $block
            )*
        }
    };
}

pub fn setup_metrics(metric_host: Option<String>, metric_port: Option<u16>) {
    let uri = metric_host;
    let port = metric_port;
    if uri.is_some() || port.is_some() {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let host = (uri.unwrap(), port.unwrap());
        let udp_sink = BufferedUdpMetricSink::from(host, socket).unwrap();
        let queuing_sink = QueuingMetricSink::from(udp_sink);
        let builder = StatsdClient::builder("das_ingester", queuing_sink);
        let client = builder.build();
        set_global_default(client);
    }
}
