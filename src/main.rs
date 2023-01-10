use server::{
    configuration::get_configuration,
    worker_loop::run_worker_until_stopped
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration().expect("Failed to read configuration.");
    configuration.tracker.generate_abi().unwrap();
    run_worker_until_stopped(configuration).await?;
    Ok(())
}
