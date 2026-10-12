# Travail avec les sous-agents

Les guides officiels OpenAI recommandent des tâches indépendantes et bornées, un contexte ciblé, un résultat attendu précis et une coordination explicite lorsque les agents partagent des fichiers : [sous-agents](https://developers.openai.com/api/docs/guides/responses-multi-agent), [coordination](https://developers.openai.com/api/docs/guides/agents-api/multi-agent).

Application dans ce dépôt, selon la demande de l'utilisateur : trois agents GPT-6 Luna avec raisonnement moyen, chacun propriétaire d'un ensemble de fichiers distinct. Ils commencent par une exploration ciblée puis lisent une spec commune qui fixe les interfaces, limites et critères de validation. Le parent tranche les dépendances, intègre les modules, relit les frontières et exécute les contrôles du produit assemblé. Les agents ne se multiplient pas et ne modifient pas les fichiers partagés sans coordination.

La validation d'un module ne vaut pas validation de l'US entière : les critères restants sont conservés dans le backlog. Les documents sont actualisés après vérification de l'intégration, puis le parent publie le lot conformément à l'autorisation de la session.
