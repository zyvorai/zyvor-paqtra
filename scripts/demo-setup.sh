#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -e

echo "🚀 Paqtra - Demo Setup"
echo "=========================="
echo ""

# Check prerequisites
echo "📋 Checking prerequisites..."

if ! command -v kubectl &> /dev/null; then
    echo "❌ kubectl not found. Please install kubectl first."
    exit 1
fi

if ! command -v cilium &> /dev/null; then
    echo "❌ cilium CLI not found. Please install cilium CLI first."
    echo "   Install: https://docs.cilium.io/en/stable/gettingstarted/k8s-install-default/"
    exit 1
fi

echo "✅ kubectl found"
echo "✅ cilium CLI found"
echo ""

# Check if cluster is available
echo "🔍 Checking Kubernetes cluster..."
if ! kubectl cluster-info &> /dev/null; then
    echo "❌ No Kubernetes cluster found."
    echo ""
    echo "Would you like to create a minikube cluster with Cilium? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo "🎪 Creating minikube cluster..."
        minikube start --network-plugin=cni --cni=false
        echo "✅ Minikube cluster created"
    else
        echo "Please create a Kubernetes cluster first."
        exit 1
    fi
fi

echo "✅ Kubernetes cluster is accessible"
echo ""

# Check if Cilium is installed
echo "🔍 Checking Cilium installation..."
if ! kubectl get pods -n kube-system -l k8s-app=cilium &> /dev/null; then
    echo "⚠️  Cilium not detected."
    echo "Would you like to install Cilium? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo "📦 Installing Cilium..."
        cilium install
        echo "⏳ Waiting for Cilium to be ready..."
        cilium status --wait
        echo "✅ Cilium installed"
    else
        echo "Please install Cilium first: cilium install"
        exit 1
    fi
else
    echo "✅ Cilium is installed"
fi

echo ""

# Deploy demo applications
echo "🎭 Deploying demo applications..."

cat <<EOF | kubectl apply -f -
---
apiVersion: v1
kind: Namespace
metadata:
  name: demo
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: frontend
  namespace: demo
spec:
  replicas: 2
  selector:
    matchLabels:
      app: frontend
  template:
    metadata:
      labels:
        app: frontend
    spec:
      containers:
      - name: nginx
        image: nginx:alpine
        ports:
        - containerPort: 80
---
apiVersion: v1
kind: Service
metadata:
  name: frontend
  namespace: demo
spec:
  selector:
    app: frontend
  ports:
  - port: 80
    targetPort: 80
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: backend
  namespace: demo
spec:
  replicas: 2
  selector:
    matchLabels:
      app: backend
  template:
    metadata:
      labels:
        app: backend
    spec:
      containers:
      - name: httpbin
        image: kennethreitz/httpbin
        ports:
        - containerPort: 80
---
apiVersion: v1
kind: Service
metadata:
  name: backend
  namespace: demo
spec:
  selector:
    app: backend
  ports:
  - port: 80
    targetPort: 80
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: client
  namespace: demo
spec:
  replicas: 1
  selector:
    matchLabels:
      app: client
  template:
    metadata:
      labels:
        app: client
    spec:
      containers:
      - name: curl
        image: curlimages/curl:latest
        command: ["sleep", "infinity"]
EOF

echo "✅ Demo applications deployed"
echo ""

# Wait for pods to be ready
echo "⏳ Waiting for pods to be ready..."
kubectl wait --for=condition=ready pod -l app=frontend -n demo --timeout=60s
kubectl wait --for=condition=ready pod -l app=backend -n demo --timeout=60s
kubectl wait --for=condition=ready pod -l app=client -n demo --timeout=60s

echo "✅ All pods are ready"
echo ""

# Generate some traffic
echo "🔀 Generating network traffic..."

CLIENT_POD=$(kubectl get pod -l app=client -n demo -o jsonpath='{.items[0].metadata.name}')

for i in {1..5}; do
    echo "  Request $i..."
    kubectl exec -n demo "$CLIENT_POD" -- curl -s frontend > /dev/null || true
    kubectl exec -n demo "$CLIENT_POD" -- curl -s backend > /dev/null || true
    sleep 1
done

echo "✅ Traffic generated"
echo ""

echo "════════════════════════════════════════"
echo "✨ Demo environment is ready!"
echo "════════════════════════════════════════"
echo ""
echo "📊 Deployed resources:"
echo "  • Namespace: demo"
echo "  • Frontend: 2 replicas (nginx)"
echo "  • Backend: 2 replicas (httpbin)"
echo "  • Client: 1 replica (curl)"
echo ""
echo "🎯 Next steps:"
echo "  1. Build the TUI: cargo build --release"
echo "  2. Run the TUI: ./target/release/paqtra"
echo "  3. Watch live flows in the TUI!"
echo ""
echo "🔄 To generate more traffic:"
echo "  kubectl exec -n demo $CLIENT_POD -- curl frontend"
echo "  kubectl exec -n demo $CLIENT_POD -- curl backend"
echo ""
echo "🧹 To cleanup:"
echo "  kubectl delete namespace demo"
echo ""
