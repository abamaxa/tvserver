# Android launcher icon

`lots-of-videos.svg` is the editable artwork; `lots-of-videos.png` is a 512px
launcher preview. The three cards and play symbol fit within Android's central
66dp safe area on a 108dp canvas.

The installed app uses vector foreground/background layers in
`gen/android/app/src/main/res/drawable*`, adaptive icon definitions in
`mipmap-anydpi-v26`, and a separate monochrome layer for Android 13+ in
`mipmap-anydpi-v33`. The density-specific PNGs support older launchers. Keep these
Android resources in sync when changing the artwork. Web and desktop icons are
separate.
