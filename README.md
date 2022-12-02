# Fetchy

Fetchy is a small package manager designed to be with simple packages and be blazingly fast at it.

It's designed to be as easy-to-use but customizable as possible.

## How does it work?

Basically, it works with a system of very small manifests where the binaries are hosted on other platforms, such as GitHub.

In a package's manifest, you can for instance specify that it's located on a GitHub repository, and that the binary to download is the file in the release following a specific pattern (regex).

This means that manifests don't need to be updated when the package itself is, but the downside is that this can break. It only works reliably for releases following a pattern, which is the very big limitation of this tool.

On the other hand, this means the manifests are extremely small, and that installation simply consists in downloading a binary. Archives such as ZIPs or tarballs are also supported.

All binaries are hosted on a separate directories and must be put in your path.

## Usage

Installation is currently made from source, requiring the [Rust toolchain](https://rustup.rs/) to be installed on your machine.

Then, run:

```shell
git clone https://github.com/ClementNerma/Fetchy
cd Fetchy
cargo install --path .

# Check if everything works correctly
fetchy -V
```

Before installing packages, you need to add a _repository_. These are small files that contain a list of packages to install. 

