use serde_json::Value;
use serde_json::json;

// --- 定义天气工具函数 ---
fn get_weather(city: &str) -> String {
    //println!("[执行工具 get_weather] 正在查询 {} 的天气...", city);
    // 这里可以接入真实的 API，现在返回模拟数据
    let wea = if yunli::util::random_f32() < 0.3 {
        (31, "晴朗")
    } else {
        (25, "大雨")
    };
    format!("{} 的天气是：{}，气温 {}°C。", city, wea.1, wea.0)
}
/*
2行描述Agent = LLM + Mem + Planing + Tool Use
环境变化 -> 模型观察 -> 模型执行 -> 重复
env = Enviroment();
while true:
    action = llm.run(system_prompt + env.state)
    env.sttate = tools.run(action)
*/

/*
Agent Loop
根据LLM 返回
若工具调用 ，则继续
若纯文本，则终止或需要输入
while not done:
    res = call_llm(messaegs)
    if res.has_tool_calls:
        results = exe_tools(res.tool_calls)
        messaegs.append(results)
    else:
        done = True
        return res
*/

async fn run_openai_agent() -> Result<String, Box<dyn std::error::Error>> {
    // 配置参数
    let api_key = std::env::var("LELLM_API_KEY").unwrap(); // 替换为你的 Key
    let base_url = format!(
        "{}{}",
        std::env::var("LELLM_BASE_URL").unwrap(),
        "/v1/chat/completions"
    );
    let model = "code";

    let user_input = "浦东今天什么天气？ 下雨推荐打伞";
    println!("用户提问: {}\n", user_input);

    let tools = vec![json!({
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "获取指定城市的天气",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "城市名称，例如：上海" }
                },
                "required": ["city"]
            }
        }
    })];

    let max_try = 15;
    let client = reqwest::Client::new();
    let mut messages = vec![
        json!({"role": "system", "content": "你是一个有用的助手。如果需要查天气，请调用工具。"}),
        json!({"role": "user", "content": user_input}),
    ];

    for i in 0..max_try {
        println!("--- 第 {} 轮思考 ---", i + 1);

        let payload = json!({
            "model": model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto"
        });

        let response: Value = client
            .post(&base_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        let choice = &response["choices"][0];
        let message = &choice["message"];
        //println!("--- message :{:?}", message);

        // 将模型回复 追加 到对话历史
        messages.push(message.clone());

        // 检查是否需要调用工具
        if let Some(tool_calls) = message["tool_calls"].as_array() {
            for tool_call in tool_calls {
                let func_name = tool_call["function"]["name"].as_str().unwrap();
                let args_str = tool_call["function"]["arguments"].as_str().unwrap();
                let args: Value = serde_json::from_str(args_str)?;

                println!("Agent 执行: {}({})", &func_name, &args);
                if func_name == "get_weather" {
                    let city = args["city"].as_str().unwrap_or("未知");
                    let result = get_weather(city);
                    println!("Agent 结果: {}", &result);

                    // 将工具执行结果存入 messages
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_call["id"],
                        "name": func_name,
                        "content": result
                    }));
                }
            }
        } else {
            // 没有工具调用，说明 Agent 已经得出结论
            println!("Agent 循环了{}轮, 完成", i + 1);
            return Ok(format!(
                "最终结果: {}",
                message["content"].as_str().unwrap_or("无内容")
            ));
        }
    }

    return Ok(format!("Agent 停止：达到最大{}轮次", max_try));
}

async fn run_claude_agent() -> Result<String, Box<dyn std::error::Error>> {
    let user_input = "浦东今天什么天气？ 下雨推荐打伞";
    println!("用户提问: {}\n", user_input);
    let res = devops_agent::agent::claude::call_with_skill("get_weather", user_input).await;
    Ok(res.unwrap())
}

#[tokio::test]
async fn test_run_agent() -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let res = run_openai_agent().await?;
    println!("{}\n耗时{:.2?}\n", res, start.elapsed());

    let start = std::time::Instant::now();
    let res = run_claude_agent().await?;
    println!("{}\n耗时{:.2?}", res, start.elapsed());
    Ok(())
}
