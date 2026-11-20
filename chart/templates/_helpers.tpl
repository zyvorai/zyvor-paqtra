{{/*
Expand the name of the chart.
*/}}
{{- define "paqtra.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "paqtra.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "paqtra.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "paqtra.labels" -}}
helm.sh/chart: {{ include "paqtra.chart" . }}
{{ include "paqtra.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "paqtra.selectorLabels" -}}
app.kubernetes.io/name: {{ include "paqtra.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
API selector labels
*/}}
{{- define "paqtra.api.selectorLabels" -}}
{{ include "paqtra.selectorLabels" . }}
app.kubernetes.io/component: api
{{- end }}

{{/*
UI selector labels
*/}}
{{- define "paqtra.ui.selectorLabels" -}}
{{ include "paqtra.selectorLabels" . }}
app.kubernetes.io/component: ui
{{- end }}

{{/*
Agent selector labels
*/}}
{{- define "paqtra.agent.selectorLabels" -}}
{{ include "paqtra.selectorLabels" . }}
app.kubernetes.io/component: agent
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "paqtra.serviceAccountName" -}}
{{- if .Values.api.serviceAccount.create }}
{{- default (printf "%s-api" (include "paqtra.fullname" .)) .Values.api.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.api.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
API image with tag
*/}}
{{- define "paqtra.api.image" -}}
{{- printf "%s:%s" .Values.api.image.repository (default .Chart.AppVersion .Values.api.image.tag) }}
{{- end }}

{{/*
UI image with tag
*/}}
{{- define "paqtra.ui.image" -}}
{{- printf "%s:%s" .Values.ui.image.repository (default .Chart.AppVersion .Values.ui.image.tag) }}
{{- end }}

{{/*
Agent image with tag
*/}}
{{- define "paqtra.agent.image" -}}
{{- printf "%s:%s" .Values.agent.image.repository (default .Chart.AppVersion .Values.agent.image.tag) }}
{{- end }}
