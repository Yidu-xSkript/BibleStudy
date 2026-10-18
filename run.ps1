# MSVC can't link on this machine (no Windows SDK), so build with the GNU toolchain + MinGW.
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo run -p bible-study @args
