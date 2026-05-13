import os
from openai import OpenAI
from pydantic import BaseModel

class CesarSpecialistAgent:
    """
    LLM Context wrapper that runs INSIDE each container.
    Takes the raw anomalies found by WreckHunterCore (math/physics formulas),
    and feeds them through the local LLM (Qwen via KoboldCPP / local LLM services)
    to perform analytical reasoning ("Is this target human or geologic?").
    """
    def __init__(self, role: str):
        self.role = role.upper()
        self.api_key = os.getenv("LLM_API_KEY", "not_needed_for_kobold")

        raw_hosts = os.getenv("LLM_HOSTS", os.getenv("LLM_HOST", "http://10.0.0.161:5001/v1"))
        if isinstance(raw_hosts, str):
            self.base_urls = [h.strip() for h in raw_hosts.split(",") if h.strip()]
        else:
            self.base_urls = list(raw_hosts)

        if not self.base_urls:
            self.base_urls = ["http://10.0.0.161:5001/v1"]

        self.client = None
        self.current_host = None
        self._init_client()

    def _init_client(self):
        for url in self.base_urls:
            try:
                self.client = OpenAI(base_url=url, api_key=self.api_key)
                self.current_host = url
                print(f"[{self.role} AGENT] Initialized LLM client on {url}")
                return
            except Exception as e:
                print(f"[{self.role} AGENT] Failed to initialize LLM client on {url}: {e}")
        print(f"[{self.role} AGENT] No available LLM endpoints from {self.base_urls}")

    def analyze_results(self, raw_data: dict, scan_context: str) -> dict:
        """
        Takes raw JSON outputs from the satellite matrices and generates
        a human-readable report / boolean validation before passing to the master orchestrator.
        """
        if not self.client:
            return {"llm_analysis": "Error: Local LLM offline.", "raw_data": raw_data}

        prompt = f"""
You are the {self.role} SPECIALIST AI for CESAROPS. 
You analyze scientific sensor readings to find submerged shipwrecks or anomalies.
You just completed a '{scan_context}' scan. Here are your raw programmatic findings:
{raw_data}

As an expert in this specific remote sensing domain, briefly evaluate the findings.
1. Are these false positives or strong candidates?
2. Why mathematically?
3. Should the master orchestrator deploy another specialized scan over this target?
Keep your answer to 1 strict scientific paragraph.
"""

        last_error = None
        for url in self.base_urls:
            try:
                print(f"[{self.role} AGENT] Querying local LLM at {url}...")
                client = OpenAI(base_url=url, api_key=self.api_key)
                response = client.chat.completions.create(
                    model="deepseek-r1-distill-qwen-7b",
                    messages=[
                        {"role": "system", "content": "You are a scientific anomaly validation agent inside a remote-sensing microservice."},
                        {"role": "user", "content": prompt}
                    ],
                    temperature=0.3,
                    max_tokens=500
                )
                analysis = response.choices[0].message.content.strip()
                self.current_host = url
                return {
                    "llm_analysis": analysis,
                    "raw_data": raw_data,
                    "llm_host": url
                }
            except Exception as e:
                print(f"[{self.role} AGENT] LLM endpoint {url} failed: {e}")
                last_error = e
                continue

        return {
            "llm_analysis": f"LLM offline or timed out on all endpoints: {last_error}",
            "raw_data": raw_data,
            "llm_host": None
        }
