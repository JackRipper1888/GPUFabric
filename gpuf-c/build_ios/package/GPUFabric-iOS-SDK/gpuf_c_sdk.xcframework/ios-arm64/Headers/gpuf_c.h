#pragma once
#ifndef GPUF_C_IOS_H
#define GPUF_C_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct llama_model;
struct llama_context;
struct gpuf_multimodal_model;

typedef void (*gpuf_status_callback)(const char *message, void *user_data);
typedef void (*gpuf_token_callback)(const char *token, void *user_data);
typedef void (*gpuf_completion_callback)(void *user_data, const char *full_text, int token_count);

int gpuf_init(void);
int gpuf_cleanup(void);
const char *gpuf_version(void);
const char *gpuf_system_info(void);

struct llama_model *gpuf_load_model(const char *model_path);
struct llama_context *gpuf_create_context(struct llama_model *model);

int gpuf_generate_final_solution_text(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    char *output_buffer,
    int output_buffer_size
);

int gpuf_generate_with_sampling(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char *output_buffer,
    int output_buffer_size,
    int32_t *token_buffer,
    int token_buffer_size
);

int gpuf_start_generation_async(
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    gpuf_token_callback on_token_callback,
    void *user_data
);

int gpuf_stop_generation(struct llama_context *context);
void llama_model_free(struct llama_model *model);
void llama_free(struct llama_context *context);

int start_remote_worker(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id
);
int set_remote_worker_model(const char *model_path);
int start_remote_worker_tasks(void);
int start_remote_worker_tasks_with_callback_ptr(gpuf_status_callback callback);
int gpuf_register_remote_worker_callback(gpuf_status_callback callback, void *user_data);
int get_remote_worker_status(char *buffer, size_t buffer_size);
int stop_remote_worker(void);

struct gpuf_multimodal_model *gpuf_load_multimodal_model(
    const char *text_model_path,
    const char *mmproj_path
);
struct llama_context *gpuf_create_multimodal_context(
    struct gpuf_multimodal_model *multimodal_model
);
int gpuf_generate_multimodal(
    struct gpuf_multimodal_model *multimodal_model,
    struct llama_context *context,
    const char *text_prompt,
    const uint8_t *image_data,
    unsigned long long image_size,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char *output_buffer,
    int output_buffer_size
);
int gpuf_generate_multimodal_stream(
    struct gpuf_multimodal_model *multimodal_model,
    struct llama_context *context,
    const char *text_prompt,
    const uint8_t *image_data,
    unsigned long long image_size,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    gpuf_token_callback on_token,
    gpuf_completion_callback on_complete,
    void *user_data
);
void gpuf_free_multimodal_model(struct gpuf_multimodal_model *multimodal_model);
bool gpuf_multimodal_supports_vision(struct gpuf_multimodal_model *multimodal_model);
int gpuf_get_multimodal_info(struct gpuf_multimodal_model *multimodal_model, bool *has_vision);

#ifdef __cplusplus
}
#endif

#endif
