# Environment

Here is information about the environment you are running in:

- Platform: {{ platform }}
- Working directory: {{ cwd }}
- Is git repo: {{ is_git_repo }}
- Git branch: {{ git_branch | default("(none)") }}
- Today's date: {{ date }}
{%- if os_version %}
- OS Version: {{ os_version }}
{%- endif %}
{%- if language_preference %}

# Language Preference

You MUST respond in {{ language_preference }}. All your responses, explanations, and communications should be in this language unless the user explicitly requests otherwise.
{%- endif %}
