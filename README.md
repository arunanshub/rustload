# Rustload

Rustload is a daemon process that prefetches binary files and shared
libraries from the hard disc to the main memory of the computer system to
achieve faster application startup time. Rustload is adaptive: it monitors
the application that the user runs, and by analyzing this data, predicts
what applications he might run in the near future, and fetches those
binaries and their dependencies into memory.

It builds a Markov-based probabilistic model capturing the correlation
between every two applications on the system. The model is then used to
infer the probability that each application may be started in the near
future. These probabilities are used to choose files to prefetch into the
main memory. Special care is taken to not degrade system performance and
only prefetch when enough resources are available.

## Features

- TODO:

## Usage

- TODO:

## Why Rust?

- Because it is easier to use and understand than C and C++.
- Because Rust programs are easier to manage.
- Because I dislike manual `free(...)` after use.
  - Because I hate unpredictable `SEGFAULT`.
- Because Rust has cleaner string handling.
- [Because C++ is as ugly as a language can get.][torvalds_cpp]

## Citation

Esfahbod, B. (2006). Preload — an adaptive prefetching daemon. Retrieved
September 18, 2021, from
<https://citeseerx.ist.psu.edu/viewdoc/download?doi=10.1.1.138.2940&rep=rep1&type=pdf>.

## License

Copyright © 2021 Arunanshu Biswas.

`rustload` is made available under the terms of either the MIT license or the
Apache License 2.0, at your option.

See the [LICENSE-APACHE][apache] and [LICENSE-MIT][mit] for license details.

[torvalds_cpp]: <http://harmful.cat-v.org/software/c++/linus>
[apache]: LICENSE-APACHE
[mit]: LICENSE-MIT
