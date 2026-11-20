################################################################################
# Paqtra — Terraform Module for AWS EKS Deployment
################################################################################

terraform {
  # Uncomment and configure for production use:
  # backend "s3" {
  #   bucket         = "paqtra-terraform-state"
  #   key            = "terraform.tfstate"
  #   region         = "us-west-2"
  #   dynamodb_table = "terraform-lock"
  #   encrypt        = true
  # }

  required_version = ">= 1.5.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.25"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 2.12"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# ─── VPC ─────────────────────────────────────────────────────

module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.0"

  name = "${var.cluster_name}-vpc"
  cidr = var.vpc_cidr

  azs             = var.availability_zones
  private_subnets = var.private_subnets
  public_subnets  = var.public_subnets

  enable_nat_gateway   = true
  single_nat_gateway   = var.environment != "production"
  enable_dns_hostnames = true
  enable_dns_support   = true

  public_subnet_tags = {
    "kubernetes.io/role/elb" = 1
  }
  private_subnet_tags = {
    "kubernetes.io/role/internal-elb" = 1
  }

  tags = var.tags
}

# ─── EKS Cluster ─────────────────────────────────────────────

module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "~> 20.0"

  cluster_name    = var.cluster_name
  cluster_version = var.kubernetes_version

  vpc_id     = module.vpc.vpc_id
  subnet_ids = module.vpc.private_subnets

  cluster_endpoint_public_access       = false
  cluster_endpoint_public_access_cidrs = var.cluster_endpoint_public_access_cidrs
  cluster_endpoint_private_access      = true

  eks_managed_node_groups = {
    paqtra = {
      name           = "paqtra-ng"
      instance_types = [var.node_instance_type]
      min_size       = var.node_min_count
      max_size       = var.node_max_count
      desired_size   = var.node_desired_count

      labels = {
        role = "paqtra"
      }
    }
  }

  tags = var.tags
}

# ─── Kubernetes Provider ─────────────────────────────────────

provider "kubernetes" {
  host                   = module.eks.cluster_endpoint
  cluster_ca_certificate = base64decode(module.eks.cluster_certificate_authority_data)
  exec {
    api_version = "client.authentication.k8s.io/v1beta1"
    command     = "aws"
    args        = ["eks", "get-token", "--cluster-name", var.cluster_name]
  }
}

provider "helm" {
  kubernetes {
    host                   = module.eks.cluster_endpoint
    cluster_ca_certificate = base64decode(module.eks.cluster_certificate_authority_data)
    exec {
      api_version = "client.authentication.k8s.io/v1beta1"
      command     = "aws"
      args        = ["eks", "get-token", "--cluster-name", var.cluster_name]
    }
  }
}

# ─── Cilium (CNI) ────────────────────────────────────────────

resource "helm_release" "cilium" {
  count = var.install_cilium ? 1 : 0

  name             = "cilium"
  repository       = "https://helm.cilium.io/"
  chart            = "cilium"
  version          = var.cilium_version
  namespace        = "kube-system"
  create_namespace = false

  set {
    name  = "hubble.relay.enabled"
    value = "true"
  }
  set {
    name  = "hubble.ui.enabled"
    value = "false"
  }
  set {
    name  = "kubeProxyReplacement"
    value = "true"
  }
  set {
    name  = "encryption.enabled"
    value = var.enable_encryption ? "true" : "false"
  }
  set {
    name  = "encryption.type"
    value = "wireguard"
  }
}

# ─── Paqtra (Helm) ───────────────────────────────────

resource "helm_release" "paqtra" {
  name             = "paqtra"
  chart            = "${path.module}/../chart"
  namespace        = var.namespace
  create_namespace = true

  values = [
    yamlencode({
      api = {
        image = {
          repository = var.api_image_repository
          tag        = var.api_image_tag
        }
        replicas = var.api_replicas
        resources = {
          requests = { memory = "256Mi", cpu = "100m" }
          limits   = { memory = "512Mi", cpu = "500m" }
        }
      }
      ui = {
        image = {
          repository = var.ui_image_repository
          tag        = var.ui_image_tag
        }
        replicas = var.ui_replicas
      }
      redis = {
        enabled = true
      }
      ingress = {
        enabled   = var.enable_ingress
        className = "alb"
        hosts     = var.ingress_hosts
      }
      monitoring = {
        enabled = var.enable_monitoring
      }
    })
  ]

  depends_on = [module.eks, helm_release.cilium]
}
