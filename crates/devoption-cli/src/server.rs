use std::{io, net::Ipv4Addr};

use axum::{Router, routing::get};
use tokio::{net::TcpListener, runtime::Builder};

use crate::pages::{self, Page};

pub fn run(port: u16) -> io::Result<()> {
    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            println!(
                "Listening on http://{} (Ctrl+C to stop)",
                listener.local_addr()?
            );
            let app = Router::new()
                .route("/", get(|| async { pages::render(Page::Home) }))
                .route("/projects", get(|| async { pages::render(Page::Projects) }))
                .route("/about", get(|| async { pages::render(Page::About) }));
            axum::serve(listener, app).await
        })
}
