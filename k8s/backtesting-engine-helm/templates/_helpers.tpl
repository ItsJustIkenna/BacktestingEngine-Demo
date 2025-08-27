{{/*
Expand the name of the chart.
*/}}
{{- define "backtesting-engine.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "backtesting-engine.fullname" -}}
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
{{- define "backtesting-engine.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "backtesting-engine.labels" -}}
helm.sh/chart: {{ include "backtesting-engine.chart" . }}
{{ include "backtesting-engine.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/component: backtesting-engine
app.kubernetes.io/part-of: trading-platform
{{- end }}

{{/*
Selector labels
*/}}
{{- define "backtesting-engine.selectorLabels" -}}
app.kubernetes.io/name: {{ include "backtesting-engine.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "backtesting-engine.serviceAccountName" -}}
{{- if and .Values.serviceAccount .Values.serviceAccount.create }}
{{- default (include "backtesting-engine.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" (and .Values.serviceAccount .Values.serviceAccount.name) }}
{{- end }}
{{- end }}

{{/*
Create the image name
*/}}
{{- define "backtesting-engine.image" -}}
{{- $registryName := .Values.image.registry -}}
{{- $repositoryName := .Values.image.repository -}}
{{- $separator := ":" -}}
{{- $termination := .Values.image.tag | toString -}}
{{- if .Values.global.imageRegistry }}
{{- $registryName = .Values.global.imageRegistry -}}
{{- end -}}
{{- printf "%s/%s%s%s" $registryName $repositoryName $separator $termination -}}
{{- end }}

{{/*
Create a default fully qualified postgresql name.
*/}}
{{- define "backtesting-engine.postgresql.fullname" -}}
{{- include "common.names.dependency.fullname" (dict "chartName" "postgresql" "chartValues" .Values.postgresql "context" $) -}}
{{- end }}

{{/*
Create the postgresql service name
*/}}
{{- define "backtesting-engine.postgresql.servicename" -}}
{{- if .Values.postgresql.enabled }}
{{- include "backtesting-engine.postgresql.fullname" . }}
{{- else }}
{{- .Values.config.database.host }}
{{- end }}
{{- end }}

{{/*
Create the postgresql port
*/}}
{{- define "backtesting-engine.postgresql.port" -}}
{{- if .Values.postgresql.enabled }}
{{- .Values.postgresql.primary.service.ports.postgresql | default 5432 }}
{{- else }}
{{- .Values.config.database.port }}
{{- end }}
{{- end }}

{{/*
Create the postgresql database name
*/}}
{{- define "backtesting-engine.postgresql.database" -}}
{{- if .Values.postgresql.enabled }}
{{- .Values.postgresql.auth.database }}
{{- else }}
{{- .Values.config.database.database }}
{{- end }}
{{- end }}

{{/*
Create the postgresql username
*/}}
{{- define "backtesting-engine.postgresql.username" -}}
{{- if .Values.postgresql.enabled }}
{{- .Values.postgresql.auth.username | default "postgres" }}
{{- else }}
{{- .Values.config.database.username }}
{{- end }}
{{- end }}

{{/*
Create a default fully qualified redis name.
*/}}
{{- define "backtesting-engine.redis.fullname" -}}
{{- include "common.names.dependency.fullname" (dict "chartName" "redis" "chartValues" .Values.redis "context" $) -}}
{{- end }}

{{/*
Create the redis service name
*/}}
{{- define "backtesting-engine.redis.servicename" -}}
{{- if .Values.redis.enabled }}
{{- printf "%s-master" (include "backtesting-engine.redis.fullname" .) }}
{{- else }}
{{- "redis" }}
{{- end }}
{{- end }}

{{/*
Create the redis port
*/}}
{{- define "backtesting-engine.redis.port" -}}
{{- if .Values.redis.enabled }}
{{- .Values.redis.master.service.ports.redis | default 6379 }}
{{- else }}
{{- 6379 }}
{{- end }}
{{- end }}

{{/*
Create the config checksum annotation
*/}}
{{- define "backtesting-engine.configChecksum" -}}
checksum/config: {{ include (print $.Template.BasePath "/configmap.yaml") . | sha256sum }}
checksum/secret: {{ include (print $.Template.BasePath "/secret.yaml") . | sha256sum }}
{{- end }}

{{/*
Create storage class
*/}}
{{- define "backtesting-engine.storageClass" -}}
{{- if .Values.global.storageClass -}}
{{- .Values.global.storageClass -}}
{{- else if .Values.persistence.storageClass -}}
{{- .Values.persistence.storageClass -}}
{{- end -}}
{{- end }}

{{/*
Validate required values
*/}}
{{- define "backtesting-engine.validateValues" -}}
{{- if not .Values.config.database.username -}}
{{- fail "config.database.username is required" -}}
{{- end -}}
{{- if not .Values.config.database.database -}}
{{- fail "config.database.database is required" -}}
{{- end -}}
{{- end }}
