# `pkg/` #

The package directory contains all the libraries that all of the command line
tools end up building ontop of. Some of these shared libraries are published
on <https://crates.io> (the rust package repository) that anyone can fetch,
some are not. Each package will have a README file that will describe if it is
available on the package repository, and has all the derivative things
available like documentation available on <https://docs.rs>.
