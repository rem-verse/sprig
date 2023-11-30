# Log #

A very small micro crate that handles setting up all of our logging
infrastructure. Even though this is just one function we don't really want to
paste it in every single little crate, and what if we ever want to change it?

***NOTE: this logging infrastructure is only available on new tools. e.g. this is
present in `bridgectl`, but NOT `findbridge`. This is to keep re-implementations
having the same output format as the original tools.***

So we create just a single crate that really only handles the one thing.
Logging. Simple, and easy. If you want to modify the logging level, simply set
the environment variable `SPRIG_LOGGING` to the logging level/filter you
would like for logging in sprig components. If you're curious about what
logging levels are allowed, [please read the documentation of the tracing crate](https://docs.rs/tracing/latest/tracing/struct.Level.html#impl-Level).

As hinted too you can specify more than levels, and instead specify full on
filters with the environment variable `SPRIG_LOGGING`, while you can read
the very deep documentation [HERE](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives)
the general idea is you can set logging for _specific_ crates, and by
separating them with commas. For example: `cdb=debug,error`, this will turn the
logging level of any log lines coming from the Cat Dev-Bridge library will be
"DEBUG", but anything besides the Cat Dev-Bridge library will be at "ERROR".
These filters should make it possible to get pretty much any level of debugging
you want from the tools without too much trouble.

## Debugging with Tokio-Console ##

*note: most users will not need this, you should really only reach for this
if you're asked too, or you know you want to dig into asynchronous things
tokio-console can help with.*

Sometimes debugging asynchronous tasks can be particularly tough. What happens
if the command just freezes because we're waiting on something, and we're not
sure what's happening? What happens if we just wanna see a top like view of
everything that's running? Well this is where [tokio-console](https://github.com/tokio-rs/console#tokio-console)
comes in. It is an external tool, that you can use to get a `top` like view
of the asynchronous tasks that are running within our single process.

To enable this simply set the environment variable `SPRIG_TOKIO_CONSOLE_ADDR`
to the network address you want the console to listen on. From this point on
tokio-console will automatically be spun up and run.

***If you are setting SPRIG_LOGGING, you must be sure to turn tokio/runtime up
to trace for tokio-console to work! If you don't specify it, it will
automatically be handled.***

## Using in Other Parts of the Codebase ##

There are really two parts of logging within the codebase, displaying logs, and
actually writing log statements to be logged somewhere. This crate should only
be used in the *first case*. For command line tools that are ***not***
re-implementations (where we need to match the output 1-to-1), you would take a
dependency on the `log` crate like: `log = { path = "../../pkg/log" }`, and
then call: `log::install_logging_handlers()?;` as the first line in your `main`
function.

When you need to write a statement to output log data, you should instead use
the [`tracing`] crate. With the helpers like [`tracing::info!`],
[`tracing::error!`], etc. You also can use the other tracing macros to create
things like spans which you may be used to in other languages.
