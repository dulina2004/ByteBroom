# Development Storage providers

Choose **Dev Storage**, discover one provider or all providers, review the locations and drives, then measure the checked rows. ByteBroom checks known locations for the current Windows user. For a provider with custom storage, enter the corresponding directory under that provider's selector. Explicit paths replace the primary default for that provider. Independent categories can have their own location rules. For example, a custom NuGet packages directory leaves HTTP and plugin discovery available.

`Home`, `Local`, and `Roaming` refer to the current Windows profile, Local AppData, and Roaming AppData. The app uses Windows known folders with environment fallbacks and uses `TEMP` for the NuGet scratch candidate.

| Provider | Windows locations and supported overrides | Official reference |
| --- | --- | --- |
| npm | `Local/npm-cache/_cacache`. Supply the cache root in the UI or use `npm_config_cache`. | [npm config](https://docs.npmjs.com/cli/v11/using-npm/config#cache) |
| pnpm | Versioned stores under `Local/pnpm/store` and `X:/.pnpm-store`. Supports `PNPM_HOME/store`, `XDG_DATA_HOME/pnpm/store`, and explicitly supplied store roots. | [pnpm store settings](https://pnpm.io/10.x/settings#storedir), [per-drive stores](https://pnpm.io/faq) |
| Yarn | `Local/Yarn/Cache` or `YARN_CACHE_FOLDER`. Modern global archives use `Home/.yarn/berry/cache` or `YARN_GLOBAL_FOLDER/cache`. | [Classic](https://classic.yarnpkg.com/lang/en/docs/cli/cache/), [modern Yarn](https://yarnpkg.com/configuration/yarnrc) |
| Bun | `Home/.bun/install/cache` or `BUN_INSTALL_CACHE_DIR`. | [Bun global cache](https://bun.com/docs/pm/global-cache) |
| Gradle | Modules, build caches, and transforms under `caches`, plus `wrapper/dists`, `daemon`, and `jdks` under `Home/.gradle` or `GRADLE_USER_HOME`. | [Gradle directories](https://docs.gradle.org/current/userguide/directory_layout.html) |
| Maven | `Home/.m2/repository` or a repository explicitly provided in the UI. Local artifacts may exist only in this repository. | [Maven configuration](https://maven.apache.org/guides/mini/guide-configuring-maven.html) |
| pip | HTTP and wheel directories under `Local/pip/Cache` or `PIP_CACHE_DIR`. | [pip caching](https://pip.pypa.io/en/stable/topics/caching/) |
| uv | `Local/uv/cache` or `UV_CACHE_DIR`. | [uv storage](https://docs.astral.sh/uv/reference/storage/) |
| Poetry | Artifacts, repository responses, and default virtualenvs under `Local/pypoetry/Cache` or `POETRY_CACHE_DIR`. Supports `POETRY_VIRTUALENVS_PATH`. | [Poetry configuration](https://python-poetry.org/docs/configuration/) |
| Conda | `Home/.conda/pkgs` and `Home/.conda/envs`. Supports `CONDA_PKGS_DIRS` and `CONDA_ENVS_PATH`. A custom UI root is a package cache. | [Conda settings](https://docs.conda.io/projects/conda/en/stable/user-guide/configuration/settings.html) |
| Cargo | `Home/.cargo` or `CARGO_HOME`. Reports `registry/cache`, `registry/index`, `registry/src`, `git/db`, and `git/checkouts`. | [Cargo home](https://doc.rust-lang.org/cargo/guide/cargo-home.html) |
| NuGet | `Home/.nuget/packages`, `Local/NuGet/v3-cache`, `TEMP/NuGetScratch`, and `Local/NuGet/plugins-cache`. Supports `NUGET_PACKAGES`, `NUGET_HTTP_CACHE_PATH`, `NUGET_SCRATCH`, and `NUGET_PLUGINS_CACHE_PATH`. | [NuGet folders](https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders) |
| Go | Compiler cache under `Local/go-build` or `GOCACHE`. Module cache under `GOMODCACHE`, the first `GOPATH/pkg/mod`, or `Home/go/pkg/mod`. A custom UI root is a module cache. | [Go environment](https://pkg.go.dev/cmd/go#hdr-Environment_variables) |
| Dart Pub | Hosted and Git packages under `Local/Pub/Cache` or `PUB_CACHE`. | [Pub environment](https://dart.dev/tools/pub/environment-variables) |
| Android SDK | `Local/Android/Sdk`, `ANDROID_HOME`, or legacy `ANDROID_SDK_ROOT`. Reports platforms, build-tools, system images by API, platform-tools, emulator binaries, NDK, and CMake. | [Android SDK Manager](https://developer.android.com/studio/intro/update), [Android variables](https://developer.android.com/tools/variables) |
| Android Virtual Devices | `ANDROID_AVD_HOME`, an `avd` child of `ANDROID_EMULATOR_HOME` or `ANDROID_USER_HOME`, or `Home/.android/avd`. Identifies `.avd` directories and their snapshots and disk images by name. | [Android variables](https://developer.android.com/tools/variables) |
| JetBrains and Android Studio | Version directories in `Local/JetBrains` and `Local/Google`. Reports `caches`, `log`, and protected `LocalHistory`. A custom root holds the version directories. | [JetBrains directories](https://www.jetbrains.com/help/idea/directories-used-by-the-ide-to-store-settings-caches-plugins-and-logs.html), [Android Studio paths](https://developer.android.com/studio/intro/studio-config) |
| VS Code and Insiders | Known cache, log, workspace, extension-state, backup, and history folders under `Roaming/Code` and `Roaming/Code - Insiders`, plus standard extension directories under `Home`. Supports manually provided user-data roots. | [VS Code user data and extensions](https://code.visualstudio.com/docs/configure/command-line#_advanced-cli-options) |
| Playwright | `Local/ms-playwright` or an absolute `PLAYWRIGHT_BROWSERS_PATH`. | [Playwright browsers](https://playwright.dev/docs/browsers#managing-browser-binaries) |
| Cypress | `Local/Cypress/Cache` or `CYPRESS_CACHE_FOLDER`. | [Cypress cache](https://docs.cypress.io/app/references/advanced-installation#Binary-cache) |
| Puppeteer | Managed browser directories under `Home/.cache/puppeteer` or `PUPPETEER_CACHE_DIR`. | [Puppeteer configuration](https://pptr.dev/api/puppeteer.configuration) |
| Hugging Face | `hub` and `datasets` under `Home/.cache/huggingface`, `HF_HOME`, or `XDG_CACHE_HOME/huggingface`. Supports direct `HF_HUB_CACHE` and `HF_DATASETS_CACHE` overrides. | [Hugging Face Hub](https://huggingface.co/docs/huggingface_hub/en/package_reference/environment_variables), [Datasets](https://huggingface.co/docs/datasets/cache) |
| PyTorch Hub | `hub` under `Home/.cache/torch`, `TORCH_HOME`, or `XDG_CACHE_HOME/torch`. A custom UI root is the Hub directory. | [PyTorch Hub](https://docs.pytorch.org/docs/stable/hub.html) |

## Interpreting results

Confidence reflects directory evidence. Risk describes consequences. ByteBroom distinguishes generally regenerable caches, shared dependencies requiring review, installed tool resources, environment state, and protected storage. Custom and environment-supplied cache locations require review. Private models, locally built packages, editor history, and virtual device data may be difficult or impossible to recover.

Measured bytes are logical file lengths. Overlapping directory rows are measured as separate, disjoint scopes, so parent rows exclude separately listed children. Unavailable, skipped, and interrupted locations can have unknown sizes. An empty measured resource shows zero. Hard links can make project and global logical totals overlap. Sparse files, compression, and virtual disk capacity further limit comparisons with physical storage. Project and global views report independent totals.

Coverage is partial for portable and configuration-defined locations, Conda installation roots, legacy layouts, and AVDs whose configuration points elsewhere. Discovery uses local drive letters and skips junctions, symbolic links, network shares, and unsupported drive aliases. The app uses metadata only and does not launch the tools it inventories. Docker Desktop, WSL, and allocated-size reporting are deferred.

## Adding a provider

Register the tool and ecosystem in `src/global_caches/model.rs`, then add a rule in `providers.rs` with its narrow default, supported overrides, category, and purpose. Prefer known child directories over scanning a tool home. Add isolated fixtures to `tests.rs` covering discovery, precedence, missing paths, and protected or ambiguous resources. Record the official location reference and any uncovered layouts here.
