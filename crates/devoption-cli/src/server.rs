use std::{io, net::Ipv4Addr};

use axum::{Router, response::Html, routing::get};
use devoption_ui::Page;
use tokio::{net::TcpListener, runtime::Builder};

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
                .route(
                    "/",
                    get(|| async { Html(devoption_ui::render(Page::Home)) }),
                )
                .route(
                    "/projects",
                    get(|| async { Html(devoption_ui::render(Page::Projects)) }),
                )
                .route(
                    "/about",
                    get(|| async { Html(devoption_ui::render(Page::About)) }),
                )
                .route(
                    "/settings/appearance",
                    get(|| async { Html(devoption_ui::render(Page::Appearance)) }),
                );
            axum::serve(listener, app).await
        })
}
