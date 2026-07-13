#sudo -u tv env LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu MOVIE_DIR=/opt/tv/movies DATABASE_URL=sqlite:/opt/tv/db/tvserver-rust.sqlite DOWNLOAD_DIR=/opt/tv/downloads /opt/tvserver/src-tauri/target/debug/retry_download_events /opt/tvserver/src-tauri/download.log --copy

 sudo -u tv env LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu MOVIE_DIR=/opt/tv/movies DATABASE_URL=sqlite:/opt/tv/db/tvserver-rust.sqlite DOWNLOAD_DIR=/opt/tv/downloads timeout 300s /opt/tvserver/src-tauri/target/debug/retry_download_events /opt/tvserver/src-tauri/download.log --copy --offset=20
