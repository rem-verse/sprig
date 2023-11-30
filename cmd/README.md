# `cmd/` #

The command directory contains all the actual binaries, and scripts that you as
a user directly end up running. This contains both the re-implementations, and
the new tools that we package together. You can look at each tools README file
to find out if it's a re-implementation, or a brand new tool. As well as how to
use it.

## Re-Implementation Differences ##

We do our best to provide a series of re-implementations of official tools that
run on any OS, and are fully compatible with any scripts/tools that expect them
to be outputting specific data. However, that doesn't mean the tools are exactly
the same. There will be differences.

### Implementation Details ###

We only promise the tools will, anything else is considered an "implementation
detail":

1. Accept the same command line arguments.
2. Display the same output given the same command line arguments.
3. Write the same files/registry keys that the official tools wrote.
4. Exit with the same exit codes.

We do this so that way folks who are used to the official toolset, can see the
same thing they've always seen, and so any scripts that expected to run a tool,
and manipulate their output continue to work. HOW we get that information to
display is different. We may not always read the same data, or send the same
packets -- if we've found a better way to get that information.

We also do not necissarily take the same amount of time that the original tools
did. A perfect example of this is `findbridge` which will always wait 10+
seconds before outputting anything, and exiting when doing `findbridge -all`.
This is really annoying, and there's actually no reason for it. The reason they
probably did this was incase they had a lot of devices responding, or some were
really really slow you could have time to process all the data. However, most
people aren't dealing with these network conditions. So really, we should only
attempt to wait when we actually are dealing with those conditions. Plus, even
when we are dealing with them, you shouldn't have to wait 10+ seconds to see
any data! We should show you the data as it comes in. This is exactly what our
`findbridge` tool does. It's significantly faster, and shows you the data as it
comes in. So you don't have to wait for everyone to respond if yours responded
quickly.

### Dependence on `cafe.bat`/`cafex_env.bat`/`mochiato` ###

Some tools (mostly those written as scripts!) have dependencies on environment
variables/files/etc. set when using `cafe.bat`, `cafex_env.bat`, aka their version
of `mochiato`. In all cases we've completely removed the dependency on
`cafe.bat`/`cafex_env.bat`/`mochiato`. Of course, they'll still work in, and
respect values set by `cafe.bat`/`cafex_env.bat`/`mochiato`, BUT each script will
check if we're in a cafe like environment, and if we aren't it will perform the
same initialization that a cafe like environment would do in order to
successfully complete your command.

This was especially important for us as some of these tools were created before
`mochiato` even existed, and we wanted to be sure they were usable by everyone.
Not to mention it just kinda sucks to be like "oh no sorry your command didn't
work because you didn't load up the entire cafe environment first". At least to
us, and this is our set of scripts god dammit.

## Adding New Tools ##

For tools that are brand new, before just adding one to this repository, and
sending us a PR -- please open up a discussion item on the repository. This way
we can discuss whether it makes sense for the community to support this new
tool, if it should be folded into another tool, etc. Fret-not even if we decide
it doesn't fit in this repository you should be able to build it yourself using
the packages we publish. ***If there's ever a package that you need in order to
make a third-party tool function please let us know. File an issue, and we'll
setup what we need to start publishing that package.***
