use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::anyhow;
use rustls::ClientConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsStream;
use crate::pdu::proxy_channel_join_request::ProxyChannelJoinRequest;
use crate::pdu::proxy_channel_join_response::ProxyChannelJoinResponse;
use crate::tls_connect::connect_to_domain;

pub struct ProxyChannel {
    recipient_tls_stream: TlsStream<TcpStream>,
    sender_tcp_stream: TcpStream,
}

impl ProxyChannel {

    
    pub async fn create_proxy_channel(secure_link_server_socket_addr: SocketAddr,
                                      secure_link_server_domain: String,
                                      tls_config: Arc<ClientConfig>, 
                                      sender_tcp_stream: TcpStream, 
                                      secure_link_connection_id: String,
                                      proxy_channel_token: String,
    ) -> Result<ProxyChannel, anyhow::Error> {

        
        
        let mut tls_stream =
            connect_to_domain(
                tls_config.clone(),
                secure_link_server_socket_addr,
                secure_link_server_domain.clone()
            )
            .await
            .unwrap();
        
        
        let proxy_channel_join_request = ProxyChannelJoinRequest::new(secure_link_connection_id, proxy_channel_token);

        let request_json = serde_json::to_string(&proxy_channel_join_request).unwrap();

        let pdu_length = (request_json.len() as u32).to_be_bytes();

        let mut proxy_channel_join_request_pdu= vec![0u8];

        proxy_channel_join_request_pdu.extend_from_slice(&pdu_length);
        proxy_channel_join_request_pdu.extend_from_slice(request_json.as_bytes());

        tls_stream.write(&proxy_channel_join_request_pdu).await?;

        let _reserved = tls_stream.read_u8().await?;
        let length = tls_stream.read_u32().await?;

        let mut message = vec![0; length as usize];

        tls_stream.read_exact(&mut message).await?;

        let channel_join_response = serde_json::from_slice::<ProxyChannelJoinResponse>(&message)?;
        
        match channel_join_response {
            ProxyChannelJoinResponse::ProxyChannelJoinConfirmed(_) => {
                
                let proxy_channel =
                    ProxyChannel {
                        recipient_tls_stream: tls_stream,
                        sender_tcp_stream
                    };

                Ok(proxy_channel)
                
            }
            ProxyChannelJoinResponse::ProxyChannelJoinDenied(_) => {
                
                Err(anyhow!("ProxyChannelJoinDenied"))
                
            }
        }

    }
    
    pub async fn run_proxy(self) -> Result<(), anyhow::Error> {

        let sender_tcp_stream = self.sender_tcp_stream;
        let recipient_tls_stream = self.recipient_tls_stream;
        
        // Split the TLS stream into its read and write halves
        let (mut recipient_tls_read, mut recipient_tls_write) = tokio::io::split(recipient_tls_stream);

        // Split the TCP stream into its read and write halves
        let (mut sender_tcp_read, mut sender_tcp_write) = tokio::io::split(sender_tcp_stream);

        // Copy sender -> recipient
        let sender_to_recipient = async {
            tokio::io::copy(&mut sender_tcp_read, &mut recipient_tls_write).await?;
            recipient_tls_write.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        };

        // Copy recipient -> sender
        let recipient_to_sender = async {
            tokio::io::copy(&mut recipient_tls_read, &mut sender_tcp_write).await?;
            sender_tcp_write.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        };

        // Run both tasks concurrently
        let _result = tokio::try_join!(sender_to_recipient, recipient_to_sender);
        
        Ok(())
        
    }
    
}