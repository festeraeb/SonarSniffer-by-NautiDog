#!/bin/bash
curl -s -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt":"The capital of France is","max_length":10,"temperature":0.3}'
