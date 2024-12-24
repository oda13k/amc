# AMC
(Connector-name-independent) Auto Monitor Configurator for X11

## Install
Clone this repository and build it by running:
```console
$ cargo build --release
````

And install it with:
```console
$ cargo install --path . --root ~/.local
```
Make sure that ~/.local is in your $PATH or alternatively, you can replace ~/.local with your desired sysroot location.

## Configuration
amc matches and configures monitors based on *setups*. 

### Setup
A setup defines a certain configuration of one or more monitors. Configuration only happens in an integral fashion, meaning that either a setup exactly matches what is plugged in and everything gets configured as specified in the setup file, or nothing gets matched and every monitor gets set to a default configuration.

amc reads all setup files that have been placed in it's configuration directory, by default: `$XDG_CONFIG_HOME/amc`. Configuration is done manually (no GUI tool).

For more information, an `example.conf` can be found in `res/` or you can learn more by running: 
```console
$ amc --help
````

## Why
The video connector names on my Lenovo Thinkpad Dock Gen 2 randomly change everytime they are unplugged. This tool configures monitors based on their EDIDs, and thus it doesn't care about which ports they are plugged into.

## License
MIT