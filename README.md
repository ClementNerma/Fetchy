# Fetchy

Fetchy is a dead-simple package manager that relies on web sources such as GitHub to fetch packages.

It is designed to be very simple to use, to not require any central platform for assets storage by enabling downloads from multiple sources, all while being fast and managing dependencies automatically.


- [Features](#features)
- [Demo](#demo)
- [Step-by-step guide](#step-by-step-guide)
- [Private packages on GitHub](#private-packages-on-github)
- [Rate limiting](#rate-limiting)

## Features

* Recursive dependencies management
* Dependencies breakage check before uninstalling
* Automatically remove unneeded dependencies when uninstalling a package
* Asset pulling from direct URL or GitHub releases

## Demo

![demo](https://github.com/user-attachments/assets/ecf3a3c8-7875-446b-81a8-307f0c5c73d9)

## Step-by-step guide

This project has pre-built binaries which can be found on the [releases page](https://github.com/ClementNerma/Fetchy/releases).

Alternatively, you can install from source using [Cargo](https://rustup.rs):

```shell
cargo install --git https://github.com/ClementNerma/Fetchy
```

Now, we need to indicate where to find the packages. Fetchy uses data sources called _repositories_ for this.

They are basically simple files that uses either JSON or the following custom format which is a lot shorter:

```fetchy
name "example-repo"
description "An example repository"
packages {
    "jumpy": GitHub "ClementNerma/Jumpy" version(TagName) {
        linux[x86_64]   "^jumpy-x86_64-unknown-linux-musl.tgz$"  archive(TarGz) { bin "^jumpy$" },
        linux[aarch64]  "^jumpy-aarch64-unknown-linux-musl.tgz$" archive(TarGz) { bin "^jumpy$" },
        windows[x86_64] "^jumpy-x86_64-pc-windows-gnu.tgz$"      archive(TarGz) { bin "^jumpy.exe$" }
    }

    "trasher": GitHub "ClementNerma/Trasher" version(TagName) {
        linux[x86_64]   "^trasher-x86_64-unknown-linux-musl.tgz$"  archive(TarGz) { bin "^trasher$" },
        linux[aarch64]  "^trasher-aarch64-unknown-linux-musl.tgz$" archive(TarGz) { bin "^trasher$" },
        windows[x86_64] "^trasher-x86_64-pc-windows-gnu.tgz$"      archive(TarGz) { bin "^trasher.exe$" }
    }
}
```

Here we have two packages: `jumpy` and `trasher`. The `GitHub` keyword indicates we want to pull them from GitHub, and the string after that is the repository (`<author name>/<repository name>`).

This is called an _extractor_. The GitHub one will pull assets from the latest non-development release published in the provided repository.

The `version(TagName)` marker indicates the package's version should be extracted from the release's tag name. This is the biggest difference with other package managers: the repository doesn't change when a package is updated. Fetchy will call GitHub's API to compare the remote version to the locally installed one when you run the `update` command.

Next we have a list of every platform there is an asset for in the releases. The strnig is a regular expression that should match the asset of that given platform.

We then describe what the asset it. Here we have an archive with the `.tar.gz` extension, containing one single binary every time. We also use regular expressions to match the files inside the archive. By default, the extracted binary will keep the name it had in the archive file, but you can also provide a new name for it.

If you want a more complete example, you can check the repository [I personally use](./examples/example.fetchy), which is a lot more complete and uses more advanced features.

For now, write this in a file somewhere, and run `fetchy add-repo <path to your file>`. It will be internally compiled, checked (any error will be reported to you) and added to the program's database.

You can now install packages using `fetchy install <package>`. To remove them, run `fetchy uninstall <package>`. That's all!

## Private packages on GitHub

Fetchy can access your private packages on GitHub if you provide it with authentication data.

First, [create a fine-grained personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-fine-grained-personal-access-token) with the most limited set of permissions (just to get permissions to call the API more frequently).

When calling Fetchy, set the `FETCHY_GITHUB_TOKEN` environment variable and it will be automatically [provided in the API calls](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api?apiVersion=2022-11-28#authenticating-with-a-personal-access-token), enabling you to use your private repositories.

## Rate limiting

GitHub's API is [heavily rate limited](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28) to prevent abuses. At the time of writing, 60 requests per hour.

But because Fetchy calls the API to get informations about the latest release (for packages which are pulled from GitHub) this can make the requests fail when installing or updating a lot of packages.

To prevent this, you need to [enable GitHub authentication](#private-packages-on-github), which will raise the API limit to 5000 requests per hour.
