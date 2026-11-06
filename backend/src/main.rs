// backend/src/main.rs
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    portfolio_backend::app::run().await
}
