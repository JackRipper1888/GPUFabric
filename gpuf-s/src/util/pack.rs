use std::sync::Arc;
use tokio::sync::Mutex;
use bytes::BytesMut;

// 内存池结构
#[derive(Clone)]
pub struct BufferPool {
    pool: Arc<Mutex<Vec<BytesMut>>>,
    buffer_size: usize,
}

impl BufferPool {
    // 创建新内存池
    pub fn new(buffer_size: usize, initial_capacity: usize) -> Self {
        let mut pool = Vec::with_capacity(initial_capacity);
        for _ in 0..initial_capacity {
            pool.push(BytesMut::with_capacity(buffer_size));
        }
        
        BufferPool {
            pool: Arc::new(Mutex::new(pool)),
            buffer_size,
        }
    }

    // 获取一个缓冲区
    pub async fn get(&self) -> BytesMut {
        let mut pool = self.pool.lock().await;
        if let Some(mut buf) = pool.pop() {
            buf.clear(); // 清空缓冲区但保留容量
            buf
        } else {
            // 如果池为空，创建新缓冲区
            BytesMut::with_capacity(self.buffer_size)
        }
    }

    // 返回缓冲区到池中
    pub async fn put(&self, mut buf: BytesMut) {
        if buf.capacity() == self.buffer_size {
            let mut pool = self.pool.lock().await;
            if pool.len() < 100 { // 限制池的大小，避免占用过多内存
                buf.clear();
                pool.push(buf);
            }
        }
        // 如果缓冲区大小不匹配或池已满，让 buf 被丢弃
    }
}
