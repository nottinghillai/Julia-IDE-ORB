# Smart Edits with Models That Don't Support Tool Calling

## Abstract

Many local language models don't support tool calling, but Zed's smart edit features work perfectly with them. This paper explains how Zed enables code editing using structured text formats (XML tags or diff syntax) instead of tool calls, making smart edits accessible to any language model.

---

## 1. How It Works

### 1.1 Core Mechanism

Smart edits don't require tool calling. Models return edits as structured text that Zed parses and applies:

**XML Format**:
```xml
<edits>
<old_text line=10>
fn process_data() { ... }
</old_text>
<new_text>
async fn process_data() -> Result<Data> { ... }
</new_text>
</edits>
```

**Diff Format**:
```diff
<<<<<<< SEARCH
fn process_data() { ... }
=======
async fn process_data() -> Result<Data> { ... }
>>>>>>> REPLACE
```

Both formats tell Zed what to change: find the old text and replace it with new text.

### 1.2 Request Construction

For models without tool support, Zed:
- Checks if model supports tools: `model.supports_tool_choice()`
- If not, omits tool definitions from request
- Builds prompt that says: "Tool calls have been disabled. You MUST start your response with <edits>."
- Sends code + instruction to model as normal text completion

The model treats this as a text generation task, not tool calling.

### 1.3 Response Processing

Zed streams the response and parses it:
1. **Accumulates text** until complete tags are detected
2. **Extracts edits** from XML or diff format
3. **Finds matches** in file using fuzzy matching
4. **Applies edits** sequentially to the buffer

---

## 2. Edit Application

### 2.1 Fuzzy Matching

The model's `old_text` might not match exactly. Zed's `StreamingFuzzyMatcher`:
- Searches file incrementally as text arrives
- Uses edit distance to find closest matches
- Expands context if match is ambiguous
- Validates match before applying

This handles whitespace differences, file changes, and duplicate code.

### 2.2 Safety Validation

Before applying, Zed validates:
- **Syntax**: Tree-sitter checks result is valid code
- **Ranges**: Edits target valid file locations
- **Content**: Old text matches actual file (within tolerance)

If validation fails, edit is rejected with error message.

---

## 3. Configuration and Usage

### 3.1 Setup

Configure non-tool models in `settings.json`:

```json
{
  "language_models": {
    "openai_compatible": {
      "Local Model": {
        "api_url": "http://localhost:8000/v1",
        "available_models": [{
          "name": "my-model",
          "capabilities": {
            "tools": false
          }
        }]
      }
    }
  }
}
```

Zed automatically uses text-based editing.

### 3.2 Workflow

**User**: Selects code, types "Convert to async"

**Zed**:
1. Builds prompt with code + instruction (no tools)
2. Sends to model
3. Model returns XML or diff format
4. Zed parses and finds matches
5. Applies edit to file

**Result**: Code edited without model needing tool support.

---

## 4. Key Insights

### 4.1 Why This Works

Code editing is fundamentally about describing changes in text. Any language model that can generate structured text can power smart edits—tool calling is optional, not required.

### 4.2 Advantages

- **Universal**: Works with any text-generating model
- **Simple**: No tool implementation needed
- **Fast**: Text-only responses are often quicker
- **Flexible**: Models can use XML or diff format

### 4.3 Limitations

- **No multi-step operations**: Can't read files or search code in single request
- **Context required upfront**: All context must be in prompt
- **Format dependency**: Models must follow XML/diff format correctly

### 4.4 Comparison

**With Tools**: Can call `read_file`, `search_code`, perform multi-step operations. More autonomous but requires tool implementation.

**Without Tools**: Returns edits as text. Simpler, works with any model, but requires all context in prompt.

**Both work**. Tool support is an enhancement, not a requirement.

---

## Conclusion

Zed's smart edit system works with models that don't support tool calling by using structured text formats. Models return edits as XML tags or diff syntax, which Zed parses and applies. This makes smart edits accessible to any language model, from small 7B models to large 70B+ models, as long as they can generate structured text.

The key insight: code editing is about describing changes in text, which any language model can do. Tool calling enables more complex workflows but isn't required for basic smart edits.

---

## References

- Edit Agent: `crates/agent/src/edit_agent.rs`
- Edit Parser: `crates/agent/src/edit_agent/edit_parser.rs`
- Edit Templates: `crates/agent/src/templates/edit_file_prompt_xml.hbs`
