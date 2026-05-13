import torch
import requests
from PIL import Image
from transformers import AutoProcessor, AutoModelForCausalLM 
import time

def test_florence2(image_url):
    print("==================================================")
    print("⛵ INITIATING WRECKHUNTER VISION AI PROOF OF CONCEPT")
    print("==================================================")
    
    # 1. Setup Device
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    torch_dtype = torch.float16 if torch.cuda.is_available() else torch.float32
    print(f"[1] Using Compute Device: {device.upper()} (Dtype: {torch_dtype})")

    # 2. Load Model & Processor
    print("[2] Downloading/Loading Microsoft Florence-2-base (this takes a moment the first time)...")
    model_id = "microsoft/Florence-2-base"
    
    # Florence-2 requires trust_remote_code=True for its custom architecture
    model = AutoModelForCausalLM.from_pretrained(
        model_id, 
        torch_dtype=torch_dtype, 
        trust_remote_code=True
    ).to(device)
    
    processor = AutoProcessor.from_pretrained(model_id, trust_remote_code=True)
    print("\n✅ Model loaded successfully!")

    # 3. Load input image
    print(f"\n[3] Fetching test image of a maritime vessel from: {image_url}")
    image = Image.open(requests.get(image_url, stream=True).raw)
    if image.mode != "RGB":
        image = image.convert("RGB")
        
    # 4. Run Object Detection for "ship" or "anomaly"
    # Florence-2 uses specific task prompts. <OD> is Object Detection, <CAPTION> is captioning.
    # We can do Region Proposal <REGION_PROPOSAL> or text-conditioned Object Detection: "<CAPTION_TO_PHRASE_GROUNDING>ship"
    
    tasks = {
        "Detailed Caption": "<DETAILED_CAPTION>",
        "Object Detection": "<OD>",
    }

    for task_name, task_prompt in tasks.items():
        print(f"\n----- Executing Task: {task_name} ({task_prompt}) -----")
        inputs = processor(text=task_prompt, images=image, return_tensors="pt").to(device, torch_dtype)
        
        start_time = time.time()
        generated_ids = model.generate(
            input_ids=inputs["input_ids"],
            pixel_values=inputs["pixel_values"],
            max_new_tokens=1024,
            early_stopping=False,
            do_sample=False,
            num_beams=3,
        )
        inference_time = time.time() - start_time
        
        generated_text = processor.batch_decode(generated_ids, skip_special_tokens=False)[0]
        parsed_answer = processor.post_process_generation(
            generated_text, 
            task=task_prompt, 
            image_size=(image.width, image.height)
        )
        
        print(f"⏱️ Inference Time: {inference_time:.2f} seconds")
        print(f"🎯 AI Output: {parsed_answer}")

if __name__ == "__main__":
    # Test on a classic image of the SS Edmund Fitzgerald
    test_image = "https://upload.wikimedia.org/wikipedia/commons/9/9a/SS_Edmund_Fitzgerald_in_1971.jpg"
    test_florence2(test_image)
