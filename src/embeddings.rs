use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingsRequest {
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingObject {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingsUsage {
    pub prompt_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingsResponse {
    pub object: String,
    pub data: Vec<EmbeddingObject>,
    pub model: String,
    pub usage: EmbeddingsUsage,
}

pub async fn handle_embeddings(
    req: EmbeddingsRequest,
    client: &reqwest::Client,
) -> Result<EmbeddingsResponse, String> {
    let voyage_key = std::env::var("VOYAGE_API_KEY").unwrap_or_default();
    if !voyage_key.is_empty() {
        tracing::info!("Embeddings: forwarding to Voyage AI");
        let model = req.model.clone().unwrap_or_else(|| "voyage-3".to_string());
        
        let input_val = match &req.input {
            serde_json::Value::String(s) => serde_json::json!([s]),
            serde_json::Value::Array(arr) => serde_json::json!(arr),
            _ => return Err("Invalid input format for embeddings: expected string or array of strings".to_string()),
        };
        
        let body = serde_json::json!({
            "input": input_val,
            "model": model
        });
        
        let resp = client.post("https://api.voyageai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", voyage_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Network error connecting to Voyage: {e}"))?;
            
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Voyage API error {status}: {text}"));
        }
        
        let parsed = resp.json::<EmbeddingsResponse>().await
            .map_err(|e| format!("Failed to parse Voyage response: {e}"))?;
        Ok(parsed)
    } else {
        tracing::info!("Embeddings: VOYAGE_API_KEY not configured, falling back to mock embeddings");
        let inputs = match &req.input {
            serde_json::Value::String(s) => vec![s.clone()],
            serde_json::Value::Array(arr) => {
                let mut vec = vec![];
                for v in arr {
                    if let Some(s) = v.as_str() {
                        vec.push(s.to_string());
                    } else {
                        return Err("Invalid array element: expected string".to_string());
                    }
                }
                vec
            }
            _ => return Err("Invalid input format for embeddings: expected string or array of strings".to_string()),
        };
        
        let model = req.model.unwrap_or_else(|| "text-embedding-3-small".to_string());
        let mut data = vec![];
        let mut total_tokens = 0;
        
        for (index, text) in inputs.iter().enumerate() {
            let tokens = text.len().div_ceil(4) + 4;
            total_tokens += tokens;
            
            let mut embedding = vec![0.0f32; 1536];
            let mut sum_sq = 0.0f32;
            for i in 0..1536 {
                let mut hash = (i as u64).wrapping_mul(31).wrapping_add(index as u64);
                for &byte in text.as_bytes() {
                    hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
                }
                let val = ((hash % 2000) as f32 / 1000.0) - 1.0;
                embedding[i] = val;
                sum_sq += val * val;
            }
            
            if sum_sq > 0.0 {
                let norm = sum_sq.sqrt();
                for val in embedding.iter_mut() {
                    *val /= norm;
                }
            }
            
            data.push(EmbeddingObject {
                object: "embedding".to_string(),
                embedding,
                index,
            });
        }
        
        Ok(EmbeddingsResponse {
            object: "list".to_string(),
            data,
            model,
            usage: EmbeddingsUsage {
                prompt_tokens: total_tokens,
                total_tokens,
            },
        })
    }
}
