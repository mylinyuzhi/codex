# Memory Files
{%- for file in files %}

## {{ file.path }}

{{ file.content | trim }}
{%- endfor %}
