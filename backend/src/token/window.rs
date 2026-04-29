/// 上下文层（System/Compressed/Structured/Linear）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layer {
    System,
    Compressed,
    Structured,
    Linear,
}

/// 上下文消息层
#[derive(Debug, Clone)]
pub struct ContextLayer {
    pub name: String,
    pub messages: Vec<String>,
    pub compressible: bool,
}

impl ContextLayer {
    /// 估算 Token 数量（按每 4 字符 = 1 token）
    pub fn token_estimate(&self) -> u32 {
        self.messages.iter().map(|m| m.len() as u32 / 4).sum()
    }
}

/// 四层上下文窗口管理器
/// 分层：System（不可压缩）/ Compressed（不可压缩）/ Structured（可压缩）/ Linear（可压缩）
#[derive(Debug)]
pub struct ContextWindow {
    system: ContextLayer,
    compressed: ContextLayer,
    structured: ContextLayer,
    linear: ContextLayer,
    max_tokens: u32,
}

impl ContextWindow {
    /// 创建上下文窗口
    pub fn new(max_tokens: u32) -> Self {
        Self {
            system: ContextLayer {
                name: "System".to_string(),
                messages: Vec::new(),
                compressible: false,
            },
            compressed: ContextLayer {
                name: "Compressed".to_string(),
                messages: Vec::new(),
                compressible: false,
            },
            structured: ContextLayer {
                name: "Structured".to_string(),
                messages: Vec::new(),
                compressible: true,
            },
            linear: ContextLayer {
                name: "Linear".to_string(),
                messages: Vec::new(),
                compressible: true,
            },
            max_tokens,
        }
    }

    /// 向指定层添加消息
    pub fn add_to_layer(&mut self, layer: Layer, message: String) {
        match layer {
            Layer::System => self.system.messages.push(message),
            Layer::Compressed => self.compressed.messages.push(message),
            Layer::Structured => self.structured.messages.push(message),
            Layer::Linear => self.linear.messages.push(message),
        }
    }

    /// 计算总 Token 数
    pub fn total_tokens(&self) -> u32 {
        self.system
            .token_estimate()
            + self.compressed.token_estimate()
            + self.structured.token_estimate()
            + self.linear.token_estimate()
    }

    /// 计算使用百分比
    pub fn usage_percent(&self) -> f32 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        (self.total_tokens() as f32 / self.max_tokens as f32) * 100.0
    }

    /// 判断是否超过指定百分比阈值
    pub fn is_over_threshold(&self, threshold_percent: f32) -> bool {
        self.usage_percent() > threshold_percent
    }

    /// 压缩 Linear 层，保留最近 keep_last 条
    pub fn compress_linear(&mut self, keep_last: usize) {
        if self.linear.messages.len() > keep_last {
            let to_keep = self
                .linear
                .messages
                .drain(self.linear.messages.len() - keep_last..)
                .collect();
            self.linear.messages = to_keep;
        }
    }

    /// 清空 Structured 层
    pub fn compress_structured(&mut self) {
        self.structured.messages.clear();
    }

    /// 按 System → Compressed → Structured → Linear 顺序拼接上下文
    pub fn build_context(&self) -> Vec<String> {
        let mut context = Vec::new();
        context.extend(self.system.messages.iter().cloned());
        context.extend(self.compressed.messages.iter().cloned());
        context.extend(self.structured.messages.iter().cloned());
        context.extend(self.linear.messages.iter().cloned());
        context
    }
}
