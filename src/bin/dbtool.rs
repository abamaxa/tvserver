use anyhow::Result;
use app_lib::{
    domain::messagebus::MessageFilter,
    entrypoints::create_context,
    services::{setup_logging, MetaDataManager, DBTOOL_LOG},
};

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging(DBTOOL_LOG);

    let context = create_context().await?;

    let metadata_manager = MetaDataManager::consume(
        context.get_repository(),
        context.get_store(),
        context.get_book_file_storer(),
        context.listen_for_messages(MessageFilter::All).await?,
        context.get_local_sender(),
        context.get_spawner(),
    );

    if let Err(err) = context.get_checker().check_video_information().await {
        tracing::error!("error checking video info: {}", err);
    }

    metadata_manager.await?;

    Ok(())
}
