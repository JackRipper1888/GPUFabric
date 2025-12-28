#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

// 假设的函数声明
extern int stop_global_worker();
extern int set_remote_worker_model(const char* model_path);
extern int start_remote_worker_tasks_with_callback_ptr(void* callback);

void test_model_switching() {
    const char* models[] = {
        "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf",
        "/data/local/tmp/models/llama-3.2-1b-instruct-q4_0.gguf",
        "/data/local/tmp/models/llama-3.2-1b-instruct-q5_0.gguf"
    };
    
    printf("🔄 Testing model switching...\n");
    
    for (int i = 0; i < 3; i++) {
        printf("\n--- Switching to model %d ---\n", i + 1);
        
        // 方案1：完全重启（你的方案）
        printf("🛑 Stopping worker...\n");
        if (stop_global_worker() != 0) {
            printf("❌ Failed to stop worker\n");
            continue;
        }
        
        printf("📦 Setting new model: %s\n", models[i]);
        if (set_remote_worker_model(models[i]) != 0) {
            printf("❌ Failed to set model\n");
            continue;
        }
        
        printf("🚀 Starting worker with callback...\n");
        if (start_remote_worker_tasks_with_callback_ptr(NULL) != 0) {
            printf("❌ Failed to start worker\n");
            continue;
        }
        
        printf("✅ Model %d switched successfully\n", i + 1);
        
        // 等待一段时间让模型稳定
        printf("⏳ Waiting for stabilization...\n");
        sleep(3);
    }
    
    printf("\n🎉 Model switching test completed!\n");
}

// 更简单的热切换测试
void test_hot_swapping() {
    const char* models[] = {
        "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf",
        "/data/local/tmp/models/llama-3.2-1b-instruct-q4_0.gguf"
    };
    
    printf("🔥 Testing hot swapping...\n");
    
    for (int i = 0; i < 2; i++) {
        printf("\n--- Hot swapping to model %d ---\n", i + 1);
        
        printf("📦 Setting new model: %s\n", models[i]);
        if (set_remote_worker_model(models[i]) == 0) {
            printf("✅ Model %d hot-swapped successfully\n", i + 1);
        } else {
            printf("❌ Failed to hot swap model %d\n", i + 1);
        }
        
        sleep(2);
    }
    
    printf("\n🎉 Hot swapping test completed!\n");
}
