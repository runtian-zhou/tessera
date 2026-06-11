# Package Compilation

Tessera package compilation starts with a deliberately small module model that
can grow without changing the core TessHIR checker contract.

## Inputs

The compiler accepts either a single `.tess` file or a package directory.

- A single file is compiled as one root module and does not include the stdlib by
  default. This preserves focused parser/typechecker tests and small examples.
- A directory is compiled as a package. If the directory has a `src/` subtree
  containing `.tess` files, that subtree is the package source root. Otherwise
  the directory itself is the source root.
- Package compilation recursively discovers `.tess` files under the source root,
  skipping hidden directories and `target/`.

## Module Names

Module paths are derived from file paths relative to the package source root.

- `src/main.tess` and `src/lib.tess` define the package root module.
- `src/math.tess` defines module `math`.
- `src/tensor/layout.tess` defines module `tensor::layout`.
- `src/tensor/mod.tess` defines module `tensor`.

Top-level items in non-root modules are qualified before type checking. For
example, `struct Point` in `src/math.tess` becomes `math::Point` in the merged
program seen by the checker. References to items declared in the same file are
qualified in the same pass, so local source can keep writing `Point`.

Cross-module dependencies must be declared explicitly with top-level `use`
items. A file that wants `math::Point`, `math::sum`, or
`std::option::Option` should write:

```tess
use math::Point;
use math::sum;
use std::option::Option;
```

After that declaration, the file can use the imported leaf names as `Point`,
`sum(...)`, and `Option::Some(...)`. A namespace import such as `use math;`
declares a dependency on that module root and allows qualified paths rooted at
`math::...`.

## Standard Library

Directory packages include the bundled stdlib by default. The stdlib is loaded
as synthetic `std::...` modules before user sources. The compiler flag
`--no-stdlib` disables this behavior.

The initial stdlib is intentionally minimal and exists to exercise package
wiring. It currently provides:

```tess
std::option::Option<T>
```

Stdlib definitions are ordinary Tessera source files after package loading.
They are parsed, span-shifted, qualified, merged, and type checked through the
same pipeline as package files.

## Checker Contract

The package loader produces one merged `Program`. It also maintains a package
source map that translates diagnostics from merged global byte spans back to the
original source file and local line/column.

The checker remains package-agnostic for now. It sees fully qualified item
names such as `math::Point` and `std::option::Option`.

## Current Limits

There is no visibility or explicit `mod` item syntax yet. The parser still has
enum-constructor syntax ambiguity for qualified expressions such as
`math::sum(...)` and `math::Point { ... }`; the checker resolves those forms as
qualified function calls or struct literals when no matching enum exists, but
package sources should prefer leaf imports for ordinary cross-module use.
