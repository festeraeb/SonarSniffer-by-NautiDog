#!/usr/bin/env python3
"""
Test KoboldCPP API endpoints
Verify local LLM inference is working
"""

import requests
import json
import time

# Xeon KoboldCPP endpoint
BASE_URL = "http://100.102.158.111:5001"

def test_models():
    """Test: Get available models"""
    print("\n" + "="*60)
    print("TEST 1: List available models")
    print("="*60)
    
    response = requests.get(f"{BASE_URL}/api/v1/models", timeout=5)
    if response.status_code == 200:
        data = response.json()
        print(f"✅ Status: {response.status_code}")
        print(f"📦 Available models:")
        for model in data.get('data', []):
            print(f"   - {model['id']}")
        return True
    else:
        print(f"❌ Error: {response.status_code}")
        return False

def test_completion():
    """Test: Code completion"""
    print("\n" + "="*60)
    print("TEST 2: Code completion (Qwen2.5 Coder)")
    print("="*60)
    
    prompt = "def fibonacci(n):"
    
    payload = {
        "prompt": prompt,
        "max_tokens": 100,
        "temperature": 0.7,
        "top_p": 0.9
    }
    
    print(f"📝 Prompt: {prompt}")
    print(f"⏳ Generating...")
    
    start = time.time()
    response = requests.post(
        f"{BASE_URL}/api/v1/completions",
        json=payload,
        timeout=30
    )
    elapsed = time.time() - start
    
    if response.status_code == 200:
        data = response.json()
        completion = data.get('choices', [{}])[0].get('text', '')
        print(f"\n✅ Status: {response.status_code}")
        print(f"⏱️  Time: {elapsed:.2f}s")
        print(f"\n🔧 Response:")
        print(f"{prompt}{completion}")
        return True
    else:
        print(f"❌ Error: {response.status_code}")
        print(f"   {response.text}")
        return False

def test_chat():
    """Test: Chat API"""
    print("\n" + "="*60)
    print("TEST 3: Chat completion (if supported)")
    print("="*60)
    
    payload = {
        "messages": [
            {
                "role": "user",
                "content": "Write a Python function to calculate factorial. Be concise."
            }
        ],
        "max_tokens": 150,
        "temperature": 0.7
    }
    
    print("💬 Testing chat API...")
    
    start = time.time()
    response = requests.post(
        f"{BASE_URL}/api/v1/chat/completions",
        json=payload,
        timeout=30
    )
    elapsed = time.time() - start
    
    if response.status_code == 200:
        data = response.json()
        message = data.get('choices', [{}])[0].get('message', {}).get('content', '')
        print(f"\n✅ Status: {response.status_code}")
        print(f"⏱️  Time: {elapsed:.2f}s")
        print(f"\n🤖 Response:")
        print(f"{message}")
        return True
    else:
        print(f"⚠️  Chat API not available (status {response.status_code})")
        print(f"   This is normal if only completions API is supported")
        return None

def test_health():
    """Test: Health check"""
    print("\n" + "="*60)
    print("TEST 0: Health check")
    print("="*60)
    
    try:
        response = requests.get(f"{BASE_URL}/api/v1/models", timeout=5)
        if response.status_code == 200:
            print(f"✅ KoboldCPP is UP and responding")
            return True
        else:
            print(f"❌ KoboldCPP returned status {response.status_code}")
            return False
    except Exception as e:
        print(f"❌ Cannot reach KoboldCPP: {e}")
        return False

def main():
    print("\n╔" + "═"*58 + "╗")
    print("║ KOBOLDCPP API TEST SUITE                                  ║")
    print("║ Xeon (100.102.158.111:5001) - Qwen2.5-Coder-32B          ║")
    print("╚" + "═"*58 + "╝")
    
    results = {}
    
    # Run tests
    results['health'] = test_health()
    if not results['health']:
        print("\n❌ Server not responding. Cannot continue.")
        return
    
    results['models'] = test_models()
    results['completion'] = test_completion()
    results['chat'] = test_chat()
    
    # Summary
    print("\n" + "="*60)
    print("TEST SUMMARY")
    print("="*60)
    for test_name, passed in results.items():
        if passed is None:
            status = "⚠️  OPTIONAL"
        elif passed:
            status = "✅ PASS"
        else:
            status = "❌ FAIL"
        print(f"{status:10} {test_name.upper()}")
    
    print("\n" + "="*60)
    print("Ready for integration with cesar_agent_llm.py!")
    print("="*60)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n\n⏹️  Test interrupted")
    except Exception as e:
        print(f"\n❌ Test error: {e}")
