# Configuration Crate Migration Guide

This guide documents breaking changes in the aggressive refactoring of the `cocode-config` crate. The refactoring prioritizes **clean architecture and type safety** over backward compatibility.

## Overview

The config crate has been significantly refactored to:
1. Remove deprecated string-based APIs in favor of type-safe `ModelSpec`
2. Split the God Object (`ConfigManager`) into focused components
3. Unify validation logic across the crate
4. Remove legacy environment variable support

## Breaking Changes

### 1. String-Based APIs Removed

All methods that work with provider/model as separate strings have been removed. Use `ModelSpec` instead.

#### `ConfigManager::current()` → `ConfigManager::current_spec()`

**Old API (❌ Removed)**:
```rust
let (provider, model) = manager.current();
println!("Current: {}/{}", provider, model);
```

**New API (✅ Required)**:
```rust
use cocode_protocol::model::{ModelRole, ModelSpec};

// Get current main model spec
let spec = manager.current_spec()?;
println!("Current: {}/{}", spec.provider, spec.model);

// Or get spec for a specific role
let spec = manager.current_spec_for_role(ModelRole::Main)?;
```

#### `ConfigManager::switch()` → `ConfigManager::switch_spec()`

**Old API (❌ Removed)**:
```rust
manager.switch("anthropic", "claude-opus-4")?;
```

**New API (✅ Required)**:
```rust
use cocode_protocol::model::ModelSpec;

let spec = ModelSpec::new("anthropic", "claude-opus-4");
manager.switch_spec(&spec)?;
```

#### `ConfigManager::current_for_role()` → `ConfigManager::current_spec_for_role()`

**Old API (❌ Removed)**:
```rust
let (provider, model) = manager.current_for_role(ModelRole::Fast);
```

**New API (✅ Required)**:
```rust
let spec = manager.current_spec_for_role(ModelRole::Fast)?;
println!("Provider: {}, Model: {}", spec.provider, spec.model);
```

#### `ConfigManager::switch_role()` → `ConfigManager::switch_role_spec()`

**Old API (❌ Removed)**:
```rust
manager.switch_role(ModelRole::Fast, "anthropic", "claude-haiku-4")?;
```

**New API (✅ Required)**:
```rust
let spec = ModelSpec::new("anthropic", "claude-haiku-4");
manager.switch_role_spec(ModelRole::Fast, &spec)?;
```

### 2. RuntimeOverrides Backward-Compat Shims Removed

The following backward-compatibility methods have been removed from `RuntimeOverrides`:

- `main()` - **Removed**
- `set_main()` - **Removed**

These were simple aliases for `ModelRole::Main` handling. Use the new `RuntimeState` API instead, or access selections directly via `current_spec_for_role()`.

### 3. Legacy Environment Variable Support Removed

Support for the `CLAUDE_CODE_*` environment variable prefix has been removed.

**Old Environment Variables (❌ No Longer Supported)**:
```bash
export CLAUDE_CODE_API_KEY=sk-...
export CLAUDE_CODE_PROVIDER=anthropic
export CLAUDE_CODE_MODEL=claude-opus-4
```

**New Environment Variables (✅ Required)**:
```bash
# Provider-specific API keys (preferred)
export COCODE_ANTHROPIC_API_KEY=sk-...
export COCODE_OPENAI_API_KEY=sk-...
export COCODE_GOOGLE_GENAI_API_KEY=...

# Or use provider.json / config.json for configuration
```

### 4. ModelSpec Type Safety

The new code uses `ModelSpec` from `cocode_protocol` for all provider/model pairs. This provides:
- **Compile-time type safety** - can't accidentally pass provider as model
- **Display name tracking** - `ModelSpec` includes `display_name` field
- **Provider type information** - `provider_type` field for capabilities

```rust
use cocode_protocol::model::ModelSpec;

let spec = ModelSpec::new("anthropic", "claude-opus-4");
println!("Provider: {}", spec.provider);          // "anthropic"
println!("Model: {}", spec.model);                 // "claude-opus-4"
println!("Display: {}", spec.display_name);        // Set during resolution
println!("Type: {:?}", spec.provider_type);        // Provider type
```

## Migration Checklist

### For Application Code

- [ ] Replace `manager.current()` with `manager.current_spec()`
- [ ] Replace `manager.switch(provider, model)` with `manager.switch_spec(&ModelSpec::new(...))`
- [ ] Replace `manager.current_for_role(role)` with `manager.current_spec_for_role(role)`
- [ ] Replace `manager.switch_role(role, provider, model)` with `manager.switch_role_spec(role, &spec)`
- [ ] Update environment variables from `CLAUDE_CODE_*` to `COCODE_*`

### For Tests

- [ ] Update assertion messages to use `ModelSpec` instead of tuples
- [ ] Use `ModelSpec::new()` for test fixtures
- [ ] Remove tests for string-based APIs (no longer exist)

## New Architecture

### ConfigStore (NEW)

Encapsulates configuration file loading, caching, and resolution:

```rust
use cocode_config::ConfigStore;

let store = ConfigStore::from_default()?;
store.reload()?;  // Reload from disk

// Internal queries (used by ConfigManager)
let info = store.resolve_model_info("anthropic", "claude-opus-4")?;
```

### RuntimeState (NEW)

Manages in-memory runtime switching:

```rust
use cocode_config::RuntimeState;
use cocode_protocol::model::ModelSpec;

let runtime = RuntimeState::new();
let spec = ModelSpec::new("anthropic", "claude-opus-4");
runtime.switch_spec_main(&spec);
```

### ApiKey Newtype (NEW)

Secure API key wrapper with redacted Debug output:

```rust
use cocode_config::ApiKey;

let key = ApiKey::new("sk-secret-key".to_string());
println!("{:?}", key);  // Prints: ApiKey([REDACTED])
println!("{}", key.expose());  // Only when explicitly needed
```

## Examples

### Before (Old API)

```rust
use cocode_config::ConfigManager;

let manager = ConfigManager::from_default()?;

// Get current model
let (provider, model) = manager.current();
println!("Using: {}/{}", provider, model);

// Switch model
manager.switch("anthropic", "claude-sonnet-4-20250514")?;

// Get model for specific role
let (fast_provider, fast_model) = manager.current_for_role(ModelRole::Fast);
println!("Fast model: {}/{}", fast_provider, fast_model);

// Switch for specific role
manager.switch_role(ModelRole::Plan, "anthropic", "claude-haiku-4")?;
```

### After (New API)

```rust
use cocode_config::ConfigManager;
use cocode_protocol::model::{ModelRole, ModelSpec};

let manager = ConfigManager::from_default()?;

// Get current model
let spec = manager.current_spec()?;
println!("Using: {}/{}", spec.provider, spec.model);

// Switch model
let new_spec = ModelSpec::new("anthropic", "claude-sonnet-4-20250514");
manager.switch_spec(&new_spec)?;

// Get model for specific role
let spec = manager.current_spec_for_role(ModelRole::Fast)?;
println!("Fast model: {}/{}", spec.provider, spec.model);

// Switch for specific role
let plan_spec = ModelSpec::new("anthropic", "claude-haiku-4");
manager.switch_role_spec(ModelRole::Plan, &plan_spec)?;
```

## Gradual Migration Strategy

If migrating a large codebase, consider this approach:

1. **Identify all call sites** using grep:
   ```bash
   grep -r "\.current()\|\.switch(\|\.current_for_role(\|\.switch_role(" \
     --include="*.rs" <your-code>
   ```

2. **Create a helper module** (temporary) to ease transition:
   ```rust
   mod compat {
       use cocode_config::ConfigManager;
       use cocode_protocol::model::{ModelRole, ModelSpec};

       impl ConfigManager {
           // Temporary wrapper for easier migration
           pub fn current_legacy(&self) -> Result<(String, String), ConfigError> {
               let spec = self.current_spec()?;
               Ok((spec.provider, spec.model))
           }
       }
   }
   ```

3. **Migrate module by module** using the new APIs
4. **Remove helper module** once migration is complete

## Questions?

Refer to the new `RuntimeState` and `ConfigStore` documentation, or see examples in the test files:
- `common/config/src/runtime.rs` - Runtime state management
- `common/config/src/store.rs` - Configuration storage and resolution
- `common/config/src/manager.test.rs` - Integration examples
