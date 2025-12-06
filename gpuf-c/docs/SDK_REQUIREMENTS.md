# SDK Method Requirements List

## 📋 Backend Provided Methods (test_long_generation.c)

✅ `gpuf_init()` - Initialize SDK  
✅ `gpuf_cleanup()` - Cleanup SDK  
✅ `gpuf_load_model(model_path)` - Load model  
✅ `gpuf_create_context(model)` - Create context  
✅ `gpuf_generate_with_sampling(...)` - Generate text  

## 🚨 Required Methods (Core Functions)

### 1. `gpuf_stop_generation(context)`
**Purpose:** Stop ongoing text generation  
**Parameters:** `llama_context* ctx`  
**Returns:** `int` (0 = success)  
**Usage:** Called when user clicks stop button

### 2. `gpuf_get_model_info(model_path)`
**Purpose:** Get model information without loading full model  
**Parameters:** `const char* model_path`  
**Returns:** `char*` (JSON format) or struct pointer  
**Should include:** File type, parameter count, context length, model size, etc.  
**Usage:** Check model quantization type, memory requirements, etc.

### 3. `gpuf_release_context(context)`
**Purpose:** Release context resources  
**Parameters:** `llama_context* ctx`  
**Returns:** `int` (0 = success)  
**Usage:** Called when unloading model, releasing memory

### 4. `gpuf_is_model_loaded(context)`
**Purpose:** Check if model is loaded  
**Parameters:** `llama_context* ctx`  
**Returns:** `int` (1 = loaded, 0 = not loaded)  
**Usage:** Check model status

## 📝 Suggested Methods (Important Functions)

### 5. `gpuf_format_chat(context, messages, template)`
**Purpose:** Format messages using chat template  
**Parameters:**
- `llama_context* ctx`
- `const char* messages` (JSON format message array)
- `const char* template` (optional, Jinja2 template string)
**Returns:** `char*` (formatted prompt string)  
**Usage:** Chat interface formatting user messages and system prompts

### 6. `gpuf_get_model_metadata(context)`
**Purpose:** Get metadata of loaded model  
**Parameters:** `llama_context* ctx`  
**Returns:** `char*` (JSON format)  
**Should include:** `size` (model size), `nParams` (parameter count), `desc` (description)  
**Usage:** Display model information, performance benchmarking

## 🔧 Enhancement Requirements for Existing Methods

### `gpuf_generate_with_sampling` needs to support:

1. **Streaming Output**
   - Add callback: `void (*on_token)(const char* token, void* user_data)`
   - Return each generated token in real-time

2. **More Sampling Parameters**
   ```c
   typedef struct {
       float temperature;
       int top_k;
       float top_p;
       float min_p;
       float repeat_penalty;
       int penalty_last_n;
       float penalty_freq;
       float penalty_present;
       int mirostat;
       float mirostat_tau;
       float mirostat_eta;
       int seed;
       // ... more parameters
   } GenerationParams;
   ```

3. **Stop Words Support**
   - Parameters: `const char** stop_words, int stop_words_count`

4. **Message Format Support**
   - Support JSON format `messages` array (not just prompt string)
   - Format: `[{"role": "user", "content": "..."}, ...]`

5. **Structured Output**
   - Parameter: `const char* json_schema` (JSON Schema string)
   - Used to force model to output specific JSON format

6. **Return More Information**
   ```c
   typedef struct {
       char* text;
       int tokens_generated;
       double time_to_first_token_ms;
       double total_time_ms;
   } GenerationResult;
   ```

**Recommended New Signature:**
```c
int gpuf_generate_with_sampling_v2(
    llama_context* ctx,
    const char* prompt_or_messages,  // JSON format prompt or messages
    GenerationParams* params,
    const char** stop_words,
    int stop_words_count,
    const char* json_schema,          // optional, for structured output
    void (*on_token)(const char* token, void* user_data),  // streaming output callback
    void* user_data,
    GenerationResult* result
);
```

## 🎯 Optional Features (if supporting multimodal)

### 7. `gpuf_init_multimodal(context, mmproj_path, use_gpu)`
**Purpose:** Initialize multimodal support (image understanding)  
**Parameters:**
- `llama_context* ctx`
- `const char* mmproj_path`
- `int use_gpu` (1 = use GPU, 0 = don't use GPU)
**Returns:** `int` (1 = success, 0 = failure)

### 8. `gpuf_is_multimodal_enabled(context)`
**Purpose:** Check if multimodal is enabled  
**Parameters:** `llama_context* ctx`  
**Returns:** `int` (1 = enabled, 0 = not enabled)

### 9. `gpuf_release_multimodal(context)`
**Purpose:** Release multimodal resources  
**Parameters:** `llama_context* ctx`  
**Returns:** `int` (0 = success)

## 💾 Optional Features (Session Cache)

### 10. `gpuf_save_session(context, path, size)`
**Purpose:** Save session cache  
**Parameters:**
- `llama_context* ctx`
- `const char* path`
- `int size` (-1 means save all)
**Returns:** `int` (number of tokens saved)

### 11. `gpuf_load_session(context, path)`
**Purpose:** Load session cache  
**Parameters:**
- `llama_context* ctx`
- `const char* path`
**Returns:** `int` (number of tokens loaded)

## 📊 Performance Testing (Optional)

### 12. `gpuf_bench(context, pp, tg, pl, nr)`
**Purpose:** Run performance benchmark  
**Parameters:**
- `llama_context* ctx`
- `int pp` (prompt processing tokens)
- `int tg` (token generation tokens)
- `int pl` (prompt length)
- `int nr` (number of runs)
**Returns:** `char*` (JSON format, containing `speedPp`, `speedTg`)

## 📋 Priority Summary

### 🔴 High Priority (Required)
1. `gpuf_stop_generation` - Stop generation
2. `gpuf_get_model_info` - Get model information
3. `gpuf_release_context` - Release context
4. `gpuf_is_model_loaded` - Check model status
5. Enhance `gpuf_generate_with_sampling` - Support streaming and more parameters

### 🟡 Medium Priority (Recommended)
6. `gpuf_format_chat` - Format chat
7. `gpuf_get_model_metadata` - Get model metadata

### 🟢 Low Priority (Optional)
8-12. Multimodal, session cache, performance testing related methods

## 📝 Usage Examples

### Current Usage Flow
```typescript
// 1. Initialize
const context = await initLlama({
  model: "/path/to/model.gguf",
  n_ctx: 2048,
  n_threads: 4,
  // ...
});

// 2. Generate text (streaming)
const result = await context.completion({
  messages: [{role: "user", content: "Hello"}],
  n_predict: 100,
  temperature: 0.7,
  // ...
}, (token) => {
  console.log("Token:", token);
});

// 3. Stop generation
await context.stopCompletion();

// 4. Release
await context.release();
```

### Required Corresponding C API
```c
// 1. Initialize
llama_model* model = gpuf_load_model("/path/to/model.gguf");
llama_context* ctx = gpuf_create_context(model);

// 2. Generate text (needs streaming support)
gpuf_generate_with_sampling_v2(
    ctx,
    "[{\"role\":\"user\",\"content\":\"Hello\"}]",  // JSON messages
    &params,
    stop_words, stop_count,
    NULL,  // json_schema
    on_token_callback, 
    user_data,
    &result
);

// 3. Stop generation
gpuf_stop_generation(ctx);

// 4. Release
gpuf_release_context(ctx);
```

