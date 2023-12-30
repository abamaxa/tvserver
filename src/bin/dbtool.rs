use anyhow::Result;
use tvserver::{entrypoints::create_context, services::{MetaDataManager, setup_logging, DBTOOL_LOG}};


#[tokio::main]
async fn main() -> Result<()> {

    setup_logging(DBTOOL_LOG);

    let context = create_context().await?;

    let metadata_manager = MetaDataManager::consume(
        context.get_repository(),
        context.get_local_receiver(),
        context.get_local_sender(),
    );

    if let Err(err) = context.get_store().check_video_information().await {
        tracing::error!("error checking video info: {}", err);
    }

    metadata_manager.await?;

    Ok(())
}
