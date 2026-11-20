################################################################################
# Paqtra — Terraform Variables
################################################################################

# ─── AWS ─────────────────────────────────────────────────────

variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-west-2"
}

variable "availability_zones" {
  description = "Availability zones"
  type        = list(string)
  default     = ["us-west-2a", "us-west-2b", "us-west-2c"]
}

# ─── VPC ─────────────────────────────────────────────────────

variable "vpc_cidr" {
  description = "VPC CIDR block"
  type        = string
  default     = "10.0.0.0/16"
}

variable "private_subnets" {
  description = "Private subnet CIDRs"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
}

variable "public_subnets" {
  description = "Public subnet CIDRs"
  type        = list(string)
  default     = ["10.0.101.0/24", "10.0.102.0/24", "10.0.103.0/24"]
}

# ─── EKS ─────────────────────────────────────────────────────

variable "cluster_name" {
  description = "EKS cluster name"
  type        = string
  default     = "paqtra-cluster"
}

variable "kubernetes_version" {
  description = "Kubernetes version"
  type        = string
  default     = "1.29"
}

variable "cluster_endpoint_public_access_cidrs" {
  description = "List of CIDR blocks allowed to access the EKS public endpoint"
  type        = list(string)
  default     = ["10.0.0.0/8"]
}

variable "node_instance_type" {
  description = "EC2 instance type for worker nodes"
  type        = string
  default     = "t3.medium"
}

variable "node_min_count" {
  description = "Minimum number of worker nodes"
  type        = number
  default     = 2
}

variable "node_max_count" {
  description = "Maximum number of worker nodes"
  type        = number
  default     = 6
}

variable "node_desired_count" {
  description = "Desired number of worker nodes"
  type        = number
  default     = 3
}

# ─── Cilium ──────────────────────────────────────────────────

variable "install_cilium" {
  description = "Install Cilium CNI via Helm"
  type        = bool
  default     = true
}

variable "cilium_version" {
  description = "Cilium Helm chart version"
  type        = string
  default     = "1.15.3"
}

variable "enable_encryption" {
  description = "Enable WireGuard encryption"
  type        = bool
  default     = false
}

# ─── Paqtra ───────────────────────────────────────────

variable "namespace" {
  description = "Kubernetes namespace for Paqtra"
  type        = string
  default     = "cilium-system"
}

variable "environment" {
  description = "Environment (dev, staging, production)"
  type        = string
  default     = "dev"
}

variable "api_image_repository" {
  description = "API Docker image repository"
  type        = string
  default     = "ghcr.io/zyvorai/paqtra-api"
}

variable "api_image_tag" {
  description = "API Docker image tag — use versioned tags in production"
  type        = string
  default     = "v1.0.0"
}

variable "ui_image_repository" {
  description = "UI Docker image repository"
  type        = string
  default     = "ghcr.io/zyvorai/paqtra-ui"
}

variable "ui_image_tag" {
  description = "UI Docker image tag — use versioned tags in production"
  type        = string
  default     = "v1.0.0"
}

variable "api_replicas" {
  description = "API pod replicas"
  type        = number
  default     = 2
}

variable "ui_replicas" {
  description = "UI pod replicas"
  type        = number
  default     = 2
}

variable "enable_ingress" {
  description = "Enable Kubernetes Ingress"
  type        = bool
  default     = false
}

variable "ingress_hosts" {
  description = "Ingress hostnames"
  type        = list(string)
  default     = ["paqtra.example.com"]
}

variable "enable_monitoring" {
  description = "Enable Prometheus ServiceMonitor"
  type        = bool
  default     = false
}

variable "tags" {
  description = "Resource tags"
  type        = map(string)
  default = {
    Project     = "paqtra"
    ManagedBy   = "terraform"
  }
}
