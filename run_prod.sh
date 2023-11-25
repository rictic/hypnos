set -e

source secrets.env

cargo build --release

# kill the previous bot, if any
kill `ps aux | grep -v grep | grep target/release/hypnos | tr -s ' ' | cut -d ' ' -f 2` || echo ''
nohup ./target/release/hypnos >./nohup.out &
tail -f nohup.out
