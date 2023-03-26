window.SIDEBAR_ITEMS = {"mod":[["__mock_MockMediaDownloader",""],["__mock_MockMediaDownloader_MediaDownloader",""],["__mock_MockMediaStorer",""],["__mock_MockMediaStorer_MediaStorer",""],["__mock_MockPlayer",""],["__mock_MockPlayer_Player",""],["__mock_MockRemotePlayer",""],["__mock_MockRemotePlayer_RemotePlayer",""],["__mock_MockTaskMonitor",""],["__mock_MockTaskMonitor_TaskMonitor",""],["__mock_MockTextFetcher",""],["__mock_MockTextFetcher_TextFetcher",""]],"struct":[["MockMediaDownloader","Provides methods for retrieving content, for instance downloading a torrent, or a URL"],["MockMediaStorer","Interface to a repository of available media files, currently implemented for the file system but could also support an S3 object store for instance."],["MockPlayer","This trait is used to provide an interface to allow the VLC player to be controlled, which was the original video player. Unlike the RemotePlayer interface it doesn’t provide an async interface and will be removed in the future."],["MockRemotePlayer","Interface to control the browser based video player via a websocket."],["MockTaskMonitor","Provides a common interface to obtain the state of a task and terminate it."],["MockTextFetcher","Provides an interface to retrieving text in the form of a String e.g. by executing an HTTP GET on a url, or opening and reading a text file."]],"trait":[["JsonFetcher","Provides an interface to retrieve JSON data and return a struct containing that data. The interface is parameterized by the type of struct to return, with much implement the DeserializeOwned trait (e.g. by deriving serde Deserialize)"],["MediaDownloader","Provides methods for retrieving content, for instance downloading a torrent, or a URL"],["MediaSearcher","Interface to a allow searching of a media source, currently implemented for the Youtube Data API and a PirateBay proxy scrapper."],["MediaStorer","Interface to a repository of available media files, currently implemented for the file system but could also support an S3 object store for instance."],["Player","This trait is used to provide an interface to allow the VLC player to be controlled, which was the original video player. Unlike the RemotePlayer interface it doesn’t provide an async interface and will be removed in the future."],["ProcessSpawner","Spawns a new os process."],["RemotePlayer","Interface to control the browser based video player via a websocket."],["StoreReaderWriter","An interface to a collection of files."],["TaskMonitor","Provides a common interface to obtain the state of a task and terminate it."],["TextFetcher","Provides an interface to retrieving text in the form of a String e.g. by executing an HTTP GET on a url, or opening and reading a text file."]],"type":[["Downloader",""],["Spawner",""],["Storer",""],["Task",""]]};