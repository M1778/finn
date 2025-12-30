# Finn

**Finn** is the official package manager and build tool for the **Fin** programming language. It provides a robust, secure, and intuitive interface for managing project dependencies, building applications, and publishing packages.

## Features

- **Dependency Management**: Easily add, remove, and update dependencies from the official registry or Git repositories.
- **Project Initialization**: Quickly scaffold new binary or library projects with sensible defaults.
- **Security First**: Built-in integrity checks and regulation validation to ensure secure dependency resolution.
- **Build System**: Integrated build commands to compile and test your Fin applications.
- **Lockfile Support**: Deterministic builds with `finn.lock`.
- **Registry Integration**: Seamless interaction with the Finn package registry.

## Installation

To install Finn, you can use the installation script (if available) or build from source:

```bash
cargo install --path .
```

*Note: Ensure you have the Fin compiler installed and available in your PATH.*

## Usage

### Creative a New Project

Initialize a new project in the current directory:

```bash
finn init
```

Or specify a name and template:

```bash
finn init --name my-app --template binary
```

### Managing Dependencies

Add a package from the registry:

```bash
finn add <package-name>
```

Add a package from a Git repository:

```bash
finn add https://github.com/username/repo.git
```

This commands will automatically update your `finn.toml` and `finn.lock` files.

### Building and Running

Build your project:

```bash
finn build
```

Run the application:

```bash
finn run
```

### Publishing

To prepare your package for distribution, ensure your `finn.toml` is correctly configured and run:

```bash
finn check
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the GPLv3 - see the [LICENSE](LICENSE) file for details.
