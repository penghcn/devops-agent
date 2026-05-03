## 架构
```
llm/
  ├── mod.rs
  ├── router.rs                    # 路由逻辑，保持在 llm 层
  ├── structured_output.rs
  └── provider/
      ├── mod.rs
      ├── anthropic.rs             # Anthropic provider
      ├── openai.rs                # OpenAI provider
      ├── config.rs                # 配置加载 + LlmConfigStore
      └── http_client.rs           # 共享 HTTP 调用逻辑
```     
## 设计
```
抽出provider mod，所有provider放在里面，比如openai_provider
高度抽象 model provider，提供对外调用统一的接口，无需关心具体llm provider 实现
统一对外 
id, //openai,anthropic ,可从环境变量DEFAULT_PROVIDER加载，统一小写化
base_url, // http://10.0.0.1:8080
api_key, //sk-kkkgk****llll
model(model_default), //默认model_flash，若无则model_pro, 若无则空
model_flash, //qwen3.6, sonnet4.5
model_pro, //deekseek v4 pro, opus4.6
chat_request //llm call抽象chatRequest，注册openai,anthropic等的具体实现类
chat_response //llm 解析抽象chatResponse，注册openai,anthropic等的具体实现类

获取配置后，组成 provider vec，并健康校验首个

使用时，参考
let provider = providers.0
let res = provider.chat_request.llm_call(prompt_message)
let res::chatResponse = provider.chat_response.parse()
if res.is_tool_call {}

```