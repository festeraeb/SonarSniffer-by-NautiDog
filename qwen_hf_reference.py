from transformers import AutoTokenizer, AutoModelForCausalLM
import torch
import json

# ================== CONFIG ==================
MODEL_NAME = "Qwen/Qwen2.5-Coder-1.5B-Instruct"
PROMPT = "Hello, I am CESARops, a search and rescue AI assistant."

# Optional: Use a full chat example
CHAT = [
    {"role": "system", "content": "You are CESARops, a helpful search and rescue AI assistant."},
    {"role": "user", "content": "What is 2+2?"},
]
# ===========================================

print("Loading tokenizer and model...")
tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME, trust_remote_code=True)
model = AutoModelForCausalLM.from_pretrained(
    MODEL_NAME,
    torch_dtype=torch.float32,  # Use float32 to match our engine
    device_map="cpu",
    trust_remote_code=True
)

# Apply Qwen2.5 chat template
text = tokenizer.apply_chat_template(CHAT, tokenize=False, add_generation_prompt=True)
print("\n=== Final Prompt Sent to Model ===")
print(text)
print("="*80)

# Tokenize
inputs = tokenizer(text, return_tensors="pt").to(model.device)
print(f"\nToken IDs: {inputs['input_ids'][0].tolist()}")
print(f"Num tokens: {inputs['input_ids'].shape[1]}")

# Forward pass (prefill)
with torch.no_grad():
    outputs = model(**inputs)
    logits = outputs.logits[0, -1, :]          # Last token logits
    probs = torch.softmax(logits, dim=-1)

# Top 10 next tokens
topk = torch.topk(probs, 10)
print("\n=== Top 10 Next Token Predictions ===")
for i, (token_id, prob) in enumerate(zip(topk.indices, topk.values)):
    token_str = tokenizer.decode(token_id)
    print(f"{i+1:2d}. Token {token_id:6d} | '{token_str}' | prob={prob:.4f}")

# Also test simple "Hello" prompt (our known-good test)
print("\n\n=== Simple 'Hello' test (no template) ===")
hello_ids = tokenizer.encode("Hello", return_tensors="pt").to(model.device)
print(f"Hello token IDs: {hello_ids[0].tolist()}")
with torch.no_grad():
    hello_out = model(hello_ids)
    hello_logits = hello_out.logits[0, -1, :]
    hello_top = torch.topk(hello_logits, 5)
    print("Top 5:")
    for tid, val in zip(hello_top.indices, hello_top.values):
        print(f"  Token {tid:6d} | '{tokenizer.decode(tid)}' | logit={val:.4f}")

# Save reference
reference = {
    "prompt": text,
    "token_ids": inputs['input_ids'][0].tolist(),
    "top_tokens": [
        {"token_id": int(tid), "token": tokenizer.decode(tid), "prob": float(p)}
        for tid, p in zip(topk.indices, topk.values)
    ],
    "greedy_token": int(logits.argmax()),
    "greedy_text": tokenizer.decode(logits.argmax()),
}

with open("/home/cesarops/wreckhunter2000-1/qwen_chat_reference.json", "w", encoding="utf-8") as f:
    json.dump(reference, f, indent=2, ensure_ascii=False)

print(f"\nReference saved to qwen_chat_reference.json")
print(f"Next token (greedy): '{tokenizer.decode(logits.argmax())}' (id={int(logits.argmax())})")
