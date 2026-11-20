################################################################################
# Paqtra — Terraform Outputs
################################################################################

output "cluster_name" {
  description = "EKS cluster name"
  value       = module.eks.cluster_name
}

output "cluster_endpoint" {
  description = "EKS cluster API endpoint"
  value       = module.eks.cluster_endpoint
}

output "cluster_version" {
  description = "Kubernetes version"
  value       = module.eks.cluster_version
}

output "vpc_id" {
  description = "VPC ID"
  value       = module.vpc.vpc_id
}

output "region" {
  description = "AWS region"
  value       = var.aws_region
}

output "kubeconfig_command" {
  description = "Command to configure kubectl"
  value       = "aws eks update-kubeconfig --region ${var.aws_region} --name ${var.cluster_name}"
}

output "paqtra_namespace" {
  description = "Namespace where Paqtra is deployed"
  value       = var.namespace
}

output "port_forward_command" {
  description = "Command to access Paqtra locally"
  value       = "kubectl -n ${var.namespace} port-forward svc/paqtra-api 9191:9191 & kubectl -n ${var.namespace} port-forward svc/paqtra-ui 3001:8080"
}
