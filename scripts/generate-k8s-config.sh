#!/bin/bash

# This script creates a kubeconfig file for the 'svc-account' service account.

if [ $# -ne 1 ]; then
    echo "Usage: $0 [service-account-name]"
    exit 1
fi

svc_account=$1
if [ "$svc_account" != "engineers-sa" ] && [ "$svc_account" != "deployers-sa" ]; then
    echo "Invalid service account. Please use 'engineers-sa' or 'deployers-sa'."
    exit 1
fi

if [ "$svc_account" = "engineers-sa" ]; then
    context_name="engineers-context"
elif [ "$svc_account" = "deployers-sa" ]; then
    context_name="deployers-context"
fi

# Get details of the 'svc-account' service account or exit if it fails
kubectl get -o yaml sa $svc_account || { echo "failed to get $svc_account group account"; exit 1; }

# Extract the name of the secret associated with the 'svc-account' service account
SA_SECRET="$(kubectl get sa $svc_account -o jsonpath='{.secrets[0].name}')" || { echo "failed to get service account secret name"; exit 1; }

# Extract and decode the service account token
SA_TOKEN="$(kubectl get secret "${SA_SECRET}" -o jsonpath='{.data.token}' | base64 -d)" || { echo "failed to decode service account token"; exit 1; }

# Get the name of the current context's cluster
CLUSTER_NAME=$(kubectl config view -o jsonpath="{.contexts[?(@.name == \"$(kubectl config current-context)\")].context.cluster}") || { echo "failed to get current context's cluster name"; exit 1; }

# Get the endpoint URL of the current context's cluster
CLUSTER_ENDPOINT=$(kubectl config view -o jsonpath="{.clusters[?(@.name == \"$CLUSTER_NAME\")].cluster.server}") || { echo "failed to get cluster endpoint"; exit 1; }

# Define the name of the new kubeconfig file
KUBECONFIG_FILE="das-k8s-config.yml"

# Create the kubeconfig file
touch $KUBECONFIG_FILE || { echo "failed to create kubeconfig file"; exit 1; }
chmod 600 $KUBECONFIG_FILE || { echo "Failed to set permissions on kubeconfig file"; exit 1; }

# Set the cluster details in the new kubeconfig file
kubectl config --kubeconfig=$KUBECONFIG_FILE set-cluster $CLUSTER_NAME --server=$CLUSTER_ENDPOINT || { echo "failed to set cluster in kubeconfig"; exit 1; }

# Set the credentials (service account token) in the new kubeconfig file
kubectl config --kubeconfig=$KUBECONFIG_FILE set-credentials $svc_account --token=$SA_TOKEN || { echo "failed to set credentials in kubeconfig"; exit 1; }

# Set the context in the new kubeconfig file
kubectl config --kubeconfig=$KUBECONFIG_FILE set-context $context_name --cluster=$CLUSTER_NAME --user=$svc_account || { echo "failed to set context in kubeconfig"; exit 1; }

# Change the current context to the new context in the new kubeconfig file
kubectl config --kubeconfig=$KUBECONFIG_FILE use-context $context_name || { echo "failed to use new context in kubeconfig"; exit 1; }

echo "Note: Ensure you add certificate-authority-data section from the admin config in the generated config or use --insecure-skip-tls-verify in your kubectl/k9s command"
echo "kubeconfig $KUBECONFIG_FILE created successfully."
