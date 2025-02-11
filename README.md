# TuxWrangler

## What is TuxWrangler

TuxWrangler is a framework designed to simplify the creation and management sets of containers, focusing on automation, scalability, and flexibility.
Unlike Packer, which excels at building single "golden images" for immutable infrastructure, TuxWrangler is tailored for environments requiring the creation and management of sets of containers.
It automates dependency updates by dynamically fetching and locking versions from container repositories, reducing manual effort and ensuring consistency.
With its centralized configuration management and integration with CI/CD pipelines, TuxWrangler streamlines complex workflows, automates testing, and publishes validated images efficiently.

TuxWrangler addresses use cases requiring dynamic updates and multiple image variations without duplicated build logic. Its ability to manage dependencies dynamically, scale configurations efficiently, and integrate seamlessly with DevOps ecosystems makes it ideal for modern, fast-paced environments.

## Building the Tool

You can build the tool from the root directory after installing rust, with

```bash
cargo install --path tuxwrangler
```

This will create a binary `tuxwrangler`.

## Usage

Start by creating a `WRANGLER.toml` file containing your container configuration.
Then call `tuxwrangler update` within the same directory.
This will create a lockfile which will inform the build.

`tuxwrangler` also provides the ability to write a multi-stage `Dockerfile` from your configuration.
To write the `Dockerfile` call `tuxwrangler write --out <DOCKERFILE_DIRECTORY>`.

Additionally, calling `tuxwrangler images` will list all `target`s in the `Dockerfile` created by `tuxwrangler` along with their `image-name` and `image-tag`.

## The Strategy

### Configuration Driven Images

TuxWrangler will use a single configuration file to build images.
To create repeatable builds, the configuration file needs to be detailed with versioning and tag specifications.  
Requiring explicit requirements for builds will increase the difficulty to scale, as well as automatically update dependencies.
To create a scalable service; TuxWrangler will use a second configuration file.
This configuration file will be the entry point for our container runtime configuration.
TuxWrangler will provide an interface to create the explicit *lock* configuration file from a generalized *config* file.

### Creating a *Lock* File

A *lock* file provides a way to explicitly list dependencies with exact versions that are stored in a project's commit history.
This ensures that builds are repeatable without requiring explicit versioning in the *config* file.

To create the *lock* file, TuxWrangler will use several version fetching strategies.
To start, TuxWrangler will have the ability to fetch versions from docker images and using Github tags/branches.
The modular design of the TuxWrangler system means it will be simple to add additional version gathering strategies if needed in the future.
The actual versions are determined from the target images ("latest", "8", "jammy") included in the *config* file using the method specified in `fetch-version`.
The templated installation instructions in the *config* file are then populated with each fetched version, and the populated installation steps are written to the *lock* file.
Next, each build definition is expanded so that an image with every combination of base/version is created. The `image-name` and `image-tag` templates are also populated for each individual build and added *lock* file.

#### Fetching Versions From Docker Images

One way of fetching actual versions for the *lock* file is through `exec`ing commands in a Docker container.
This enables automatically tracking `latest` images to determine the newest version as well as any tag naming conventions the distro uses.

The following is a sample Docker versioning configuration for ubuntu images:

```toml
# The version fetching method
type = "docker"
# The image that a container should be created from
# The `{{version}}` template will correspond to each of the `versions` in the base/features
# configuration
image = "ubuntu:{{version}}"
# The command that should be executed to determine the version
# Note: Only the last line of output will be used to detemine the version
command = [
            # Use bash for execution
            "/bin/bash", 
            "-c", 
            # Find the version listed in /etc/os-release and print that to stdout
            "grep VERSION= /etc/os-release | sed -e \"s/^VERSION=//\" | xargs echo -n"
        ]
```

The following diagram shows how the actual version is fetched using the config above for `version=jammy`:

![Diagram of control flow for docker version fetching](docs/images/VUDocker.png)

#### Fetching Versions From GitHub

Another method of fetching actual versions for the *lock* file is through GitHub branches or tags.
This enables automatically tracking `*` version to determine the newest version.
As well as any versioning scheme used by the distro for tagging/branch ("X.x.y.Final", "X.*.*.Final", "X", "X.x").

The following is a sample GitHub versioning configuration for Amazon Corretto images:

```toml
# The version fetching method
type = "github"
# The org containing the project contianing the versioning tags/branches resides in
org = "corretto"
# The project containing the versioning tags/branches
# Templating can be used if the project's name is partially determined by the version.
# In this case, the corretto project is named by the major Java version ({{versions.0}})
project = "corretto-{{versions.0}}"
# The feature that should be used to collect versions (options: tags (default), branches)
version-from = "tags"
```

A local cache is used to reduce the required api calls to GitHub to avoid rate limiting.

### Making Changes to Images

To change the installations of images, add new features, or add new target versions for a feature, the *config* file should be updated.
Once changes are made TuxWrangler can update the *lock* file for the config that will be used for image builds.

### Nightly Builds

Enabling nightly builds will be nearly trivial with TuxWrangler.
Daily, a *lock* file will be created for the version controlled *config* file.
This update will ensure that all builds contain the newest version of each base/feature that is defined in the *config* file.
After the *lock* file is generated, TuxWrangler will create all images.
Once the images are built they will be uploaded to an artifact store of choice.

## Configuration

### Defining a Base

A base is defined with target versions, a package manager (apt, yum), versioning-tags and an image template.
A version fetching strategy can also be provided to refine the versioning and tagging.

```toml
# Define a new base
[[base]]
# Name the base (this will be used when creating build definitions)
name = "ubuntu"
# Specify the versions that should be targeted (this will replace `{{version}}` in fetch-version templates)
versions = ["jammy", "focal"]
# The package manager for this os (used for determine installation of features)
package-manager = "apt"
# A docker safe tag that should be used for this base
version-tag = "ubuntu-{{versions.0}}.{{versions.1}}.{{versions.2}}"
# The image that should be used as the actual base image
# The templating for image and tags are based on the actual version that is fetched
image = "ubuntu:{{versions.0}}.{{versions.1}}"
# Define the version fetching for the current base, Docker and Github are currently supported
[base.fetch-version]
# The version fetching strategy that will be used (docker|github)
type = "docker"
# The docker image that the following command will be execed on
# The `{{version}}` in this template references each `version` in the `versions` field for this base
image = "ubuntu:{{version}}"
# The command that should be run to determine the actual version
# The version fetched this way is used to populate `version-tags` template
# This specific command fetches version in the form `X.x.y LTS (<CODENAME> <FLAVOR>)
# The version is parsed as versions.0=X, versions.1=x, versions.2=y, versions.3=LTS, versions.4=<CODENAME>, versions.5=<FLAVOR>
command = [
    "/bin/bash", 
    "-c", 
    "grep VERSION= /etc/os-release | sed -e \"s/^VERSION=//\" | xargs echo -n"]
```

### *Lock*ed Base Representation

The above definition for the base `ubuntu` is expanded as follows in a *lock* file

```toml
# Define a base for name = ubuntu, version = focal
[[base]]
# The base name
name = "ubuntu"
# The actual version yielded by `fetch-version`
version = "20.04.6 LTS (Focal Fossa)"
# The actual image that will be used
image = "ubuntu:20.04"
# The package manager
package_manager = "apt"
# The tag that will be used for this build stage
tag = "ubuntu-20.04.6"

# To help with reproducibility, tuxwrangler also uses the digest (if possible) for the base image.
[base.identifier]
type = "Digest"
digest = "sha256:0e5e4a57c2499249aafc3b40fcd541e9a456aab7296681a3994d631587203f97"

# Define a base for name = ubuntu, version = jammy
[[base]]
name = "ubuntu"
version = "22.04.4 LTS (Jammy Jellyfish)"
image = "ubuntu:22.04"
package_manager = "apt"
tag = "ubuntu-22.04.4"

# The digest for the ubuntu-jammy image.
[base.identifier]
type = "Digest"
digest = "sha256:0e5e4a57c2499249aafc3b40fcd541e9a456aab7296681a3994d631587203f97"
```

### Defining a Feature

Features include anything that should be installed to the base.
This includes certs and runtimes as well as application servers.
Features are defined similarly to bases with additional configuration for installation.

```toml
# Define a new feature
[[feature]]
# The name of the feature
name = "corretto"
# The target versions for the feature
versions = ["21", "17", "11"]
version-tag = "corretto-{{versions.0}}"
# Define the strategy for fetching the current feature
[feature.fetch-version]
# Use github tags to determine the version
type = "github"
# The github organization the project is within
org = "corretto"
# The github project (This can be templated using the defined `versions`)
project = "corretto-{{versions.0}}"
# Define the installation strategy for the current feature
[feature.step]
# Install this feature using rpm packages
method = "rpm"
# Define the script for installing packages with yum package manager
[feature.step.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    # Templates work in installation instructions too
    "yum install -y java-{{version0}}-amazon-corretto-devel",
]
# Define the script for installing packages with apt package manager
[feature.step.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    "echo \"deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main\" | tee /etc/apt/sources.list.d/corretto.list",
    "apt-get update",
    "apt-get install -y java-{{version0}}-amazon-corretto-jdk"
]
```

### *Lock*ed Feature Representation

The above definition for the base `corretto` is expanded as follows in a *lock* file.
The schema is identical to the base schema with the addition of installation instructions.
A sample *lock* representation of `corretto` can be found in [Appendix C](#appendix-c-lock-representation-for-corretto)

### Defining Features with Local Dependencies

TuxWrangler also provides and interface for bringing in local dependencies for image builds.
Suppose there is a script `hello-world.sh` that needs to be added.
The following configuration will add the `hello-world.sh` script to the image.

```toml
[[feature]]
name = "hello-script"
versions = ["1"]
# Copy the script
[[feature.step]]
method = "docker"
commands = [
    "COPY hello-world.sh /tmp/hello-world.sh"
]
dependencies = ["hello-world.sh"]
```

The `dependencies` field works for both individual files as well as nested directories.

### Build Definitions

Defining builds in the *config* file is designed to be extremly simple, and scalable.
A build is defined as follows:

```toml
# Define a build
[[build]]
# The bases that the features should be build on
bases = ["ubuntu", "debian"]
# The groups of features for this build
features = [
    # Each build should have exactly 1 of "corretto" or "temurin" installed
    [ "corretto", "temurin"], 
    # Each build should have exactly 1 of "wildfly" or "tomcat" installed
    ["wildfly", "tomcat"],
]
# The naming scheme for this set of image (tagging is supported)
image-name = "java"
image-tag = "{{#if corretto}}{{corretto.version}}-corretto{{else}}{{temurin.version}}-temurin{{/if}}-{{base.name}}-{{date}}"
```

### Builds in *Lock* file

The *lock* version contains the configuration for a single image that should be build and includes all tags that should be included for the build.
Only one of the builds from above are included below since the configuration defines at least 32 different images(["ubuntu-jammy", "ubuntu-focal", "debian-bookworm", "debian-bullseye] X ["corretto-11", "corretto-17", "corretto-21", "temurin"] X ["wildfly", "tomcat"]).

```toml
# Define a new build
[[build]]
# The target stage for this build
target = "ubuntu-22.04-corretto-21-wildfly-1.31.0"
# The name this image should recieve
image_name = "java"
# The tag for this image
image_tag = "21.0.3.9.1-corretto-ubuntu-25-01-07"
# The base this set of features will be built on
[build.base]
name = "ubuntu"
version = "22.04.4 LTS (Jammy Jellyfish)"
# A feature that should be installed to the base
[[build.features]]
name = "corretto"
version = "21.0.3.9.1"
# Another feature that should be installed to the base
[[build.features]]
name = "wildfly"
version = "1.31.0"
```

## Appendix

### Appendix A: Targeting Single Version for Build

Image builds can also be locked to specific versions of features.
The following creates a build for only corretto-8 on ubuntu jammy even if other versions were defined.
The versions included in this configuration must match the versions defined in `versions` for the base/feature.

```toml
[[build]]
bases = [{name = "ubuntu", version = "jammy"}]
features = [{name = "corretto", version = "8"}]
```

### Appendix B: Defining Features with Version Specific Installation

It is possible that templating based on the version is not strong enough.
The config file supports separating features.
The following shows how the installation for corretto-8 differs from other versions.

```toml
[[feature]]
name = "corretto"
versions = ["21", "17", "11"]
version-tags = ["corretto", "corretto-{{version0}}", "corretto-{{version}}", "corretto-{{version0}}.{{version1}}"]
[feature.fetch-version]
type = "github"
org = "corretto"
project = "corretto-{{version0}}"
[feature.installation]
method = "rpm"
[feature.installation.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-{{version0}}-amazon-corretto-devel",
]
[feature.installation.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    "echo \"deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main\" | tee /etc/apt/sources.list.d/corretto.list",
    "apt-get update",
    # Versioned with major version only
    "apt-get install -y java-{{version0}}-amazon-corretto-jdk"
]
[[feature]]
name = "corretto"
versions = ["8"]
version-tags = ["corretto-{{version0}}", "corretto-{{version}}", "corretto-{{version0}}.{{version1}}"]
[feature.fetch-version]
type = "github"
org = "corretto"
project = "corretto-{{version0}}"
[feature.installation]
method = "rpm"
[feature.installation.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-1.{{version0}}.0-amazon-corretto-devel",
]
[feature.installation.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    "echo \"deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main\" | tee /etc/apt/sources.list.d/corretto.list",
    "apt-get update",
    # Has the form 1.X.0 for version installation
    "apt-get install -y java-1.{{version0}}.0-amazon-corretto-jdk"
]
```

### Appendix C: *Lock* representation for `corretto`

```toml
[[feature]]
name = "corretto"
tag = "corretto-11.0.25.9.1"
version = "11.0.25.9.1"

[[feature.step]]
type = "actual"
method = "docker"
commands = ["ENV JAVA_HOME=/usr/lib/jvm/java-11-amazon-corretto"]
dependencies = []

[feature.step.copy]

[[feature.step]]
type = "actual"
method = "rpm"

[feature.step.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    'echo "deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main" | tee /etc/apt/sources.list.d/corretto.list',
    "apt-get update",
    "apt-get install -y java-11-amazon-corretto-jdk=1:11.0.25.9-1",
    "rm -rf /usr/lib/jvm/java-11-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-11-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-11-amazon-corretto-devel-1:11.0.25.9-1",
    "rm -rf /usr/lib/jvm/java-11-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-11-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.copy]

[[feature]]
name = "corretto"
tag = "corretto-17.0.13.11.1"
version = "17.0.13.11.1"

[[feature.step]]
type = "actual"
method = "docker"
commands = ["ENV JAVA_HOME=/usr/lib/jvm/java-17-amazon-corretto"]
dependencies = []

[feature.step.copy]

[[feature.step]]
type = "actual"
method = "rpm"

[feature.step.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    'echo "deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main" | tee /etc/apt/sources.list.d/corretto.list',
    "apt-get update",
    "apt-get install -y java-17-amazon-corretto-jdk=1:17.0.13.11-1",
    "rm -rf /usr/lib/jvm/java-17-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-17-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-17-amazon-corretto-devel-1:17.0.13.11-1",
    "rm -rf /usr/lib/jvm/java-17-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-17-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.copy]

[[feature]]
name = "corretto"
tag = "corretto-21.0.5.11.1"
version = "21.0.5.11.1"

[[feature.step]]
type = "actual"
method = "docker"
commands = ["ENV JAVA_HOME=/usr/lib/jvm/java-21-amazon-corretto"]
dependencies = []

[feature.step.copy]

[[feature.step]]
type = "actual"
method = "rpm"

[feature.step.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    'echo "deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main" | tee /etc/apt/sources.list.d/corretto.list',
    "apt-get update",
    "apt-get install -y java-21-amazon-corretto-jdk=1:21.0.5.11-1",
    "rm -rf /usr/lib/jvm/java-21-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-21-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-21-amazon-corretto-devel-1:21.0.5.11-1",
    "rm -rf /usr/lib/jvm/java-21-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-21-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.copy]

[[feature]]
name = "corretto"
tag = "corretto-8.432.06.1"
version = "8.432.06.1"

[[feature.step]]
type = "actual"
method = "docker"
commands = ["ENV JAVA_HOME=/usr/lib/jvm/java-1.8.0-amazon-corretto"]
dependencies = []

[feature.step.copy]

[[feature.step]]
type = "actual"
method = "rpm"

[feature.step.yum]
script = [
    "rpm --import https://yum.corretto.aws/corretto.key",
    "curl -L -o /etc/yum.repos.d/corretto.repo https://yum.corretto.aws/corretto.repo",
    "yum install -y java-1.8.0-amazon-corretto-devel-1:1.8.0_432.b06-1",
    "rm -rf /usr/lib/jvm/java-8-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-8-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.apt]
script = [
    "apt-get clean && apt-get update && apt-get install -y wget gpg",
    "wget -O - https://apt.corretto.aws/corretto.key | gpg --dearmor -o /usr/share/keyrings/corretto-keyring.gpg",
    'echo "deb [signed-by=/usr/share/keyrings/corretto-keyring.gpg] https://apt.corretto.aws stable main" | tee /etc/apt/sources.list.d/corretto.list',
    "apt-get update",
    "apt-get install -y java-1.8.0-amazon-corretto-jdk=1:8.432.06-1",
    "rm -rf /usr/lib/jvm/java-8-amazon-corretto/lib/src.zip",
    'echo "export JAVA_HOME=/usr/lib/jvm/java-1.8.0-amazon-corretto" > /etc/profile.d/javahome.sh ',
]

[feature.step.copy]

[[feature.step]]
type = "actual"
method = "docker"
commands = ["ENV JAVA_HOME=/usr/lib/jvm/java-8-amazon-corretto"]
dependencies = []

[feature.step.copy]
```
