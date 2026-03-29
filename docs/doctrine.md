# Fawx Doctrine

These are the architectural principles that govern how Fawx is built. They are not guidelines. They are the rules the codebase follows. Code that violates them gets refactored.

---

## Everything Describes Itself

A tool knows its name, its parameters, its category, its cacheability, its journal behavior, and its permission requirements. You don't ask a registry what a tool is. You ask the tool.

A skill knows the same things. A provider knows the same things. The contract is identical at every level. The engine works with the contract, never with the identity.

If you're writing a match on a name string, you're encoding knowledge about a component somewhere other than the component itself. That knowledge will drift, because it has no relationship to the thing it describes. The component changes; the match doesn't. The system lies.

---

## Three Rules

### 1. Components declare, systems discover.

A tool declares its behavior through a trait. The system discovers tools by iterating trait objects. No static registries. No name-to-behavior lookup tables.

When you add a new tool, you implement a trait. The system picks it up. You don't touch a dispatcher, a classifier, a category map, or a cache policy function. Those behaviors are on the trait, and the tool provides them.

### 2. Same shape at every scale.

A tool, a skill, a provider, a perception module: all satisfy the same structural contract.

`describe → execute → classify`

If you prefer classical pattern names, this is the Composite Pattern applied architecturally. A single tool and a composite subsystem present the same interface to the layer above.

The engine doesn't know which level it's talking to and doesn't need to. A single tool and an entire skill subsystem look the same from above. This is how Fawx stays extensible: new capabilities plug in at any level without changing the levels above or below.

Fawx is built like a fractal. The same pattern repeats at every scale. Composite is the local pattern; fractal architecture is the system-wide rule.

### 3. If you're matching on a name, you're in the wrong layer.

Name strings are implementation details of individual components. Systems that classify by name are doing the component's job badly. Push the classification to the component and let the trait carry it.

A match on a tool name in a dispatcher is a sign that the tool's behavior leaked out of the tool. A match on a provider name in a metadata function is a sign that the provider's capabilities leaked out of the provider. Put the knowledge where it belongs: on the thing it describes.

---

## Why This Matters

These rules exist because Fawx broke them and paid for it.

The engine had five separate string registries mapping tool names to behaviors: dispatch, caching, journaling, permissions, and proposals. They drifted out of sync. The shell tool (`run_command`) was missing from three of them. The journal reported zero actions when tools were actively running. The kernel-blind protection didn't recognize the engine's own shell command. Five systems, five copies of the truth, three of them wrong.

The fix wasn't to update the registries. The fix was to make the registries unnecessary. Tools describe themselves. The system asks them. There is one source of truth and it lives on the component.

This is how elegant software is built. Not by enumerating what exists, but by defining what anything can be.
