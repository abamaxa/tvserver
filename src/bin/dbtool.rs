use anyhow::Result;
use tvserver::{domain::messagebus::MessageFilter, entrypoints::create_context, services::{setup_logging, MetaDataManager, DBTOOL_LOG}};


#[tokio::main]
async fn main() -> Result<()> {

    setup_logging(DBTOOL_LOG);

    let context = create_context().await?;

    let metadata_manager = MetaDataManager::consume(
        context.get_repository(),
        context.listen_for_messages("MetaDataManager", MessageFilter::All).await?,
        context.get_local_sender(),
    );

    if let Err(err) = context.get_checker().check_video_information().await {
        tracing::error!("error checking video info: {}", err);
    }

    metadata_manager.await?;

    Ok(())
}
