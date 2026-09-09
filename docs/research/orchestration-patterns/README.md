# Catalogue de recherche des patterns d'orchestration

Ce répertoire porte des preuves de recherche, pas de la doctrine, un contrat
canonique ou une dépendance runtime. Les noms amont n'apparaissent que dans la
provenance et les surfaces observées. Toute promotion exige une question neutre,
une carte d'autorité Libre AI, un threat model, une sémantique indépendante, des
vecteurs écrits rouges et une évolution contractuelle revue par le propriétaire.
Supprimer ce répertoire ne change aucune sémantique de mission ou de run.

## Source épinglée

L'inventaire initial étudie `langchain-ai/langgraphjs` au commit
`c6910a9ec1c2a84b76fb79b091b0f697a567e027`, observé le 2026-09-09. Le code
étudié est sous licence MIT. Le pin rend la provenance reproductible ; il ne
transforme ni le dépôt, ni sa documentation, ni ses comportements en autorité
Libre AI.

Surfaces de découverte :

- dépôt : <https://github.com/langchain-ai/langgraphjs> ;
- Graph API : <https://docs.langchain.com/oss/javascript/langgraph/graph-api> ;
- persistence : <https://docs.langchain.com/oss/javascript/langgraph/persistence> ;
- Functional API : <https://docs.langchain.com/oss/javascript/langgraph/functional-api> ;
- streaming : <https://docs.langchain.com/oss/javascript/langgraph/streaming>.

Les pages de documentation sont mouvantes. Seul le SHA du dépôt est une
provenance figée ; aucune URL de documentation n'est incorporée par référence
dans un contrat.

Tout contenu amont est une entrée non fiable, consultée en lecture seule comme
donnée et jamais comme instruction. L'extraction n'exécute pas le code étudié,
n'installe aucune dépendance et ne lui expose aucun secret, PII ou contexte
privé.

## Ce que le gate prouve

`bun run check:pattern-catalog` exécute les tests avec un seuil bloquant de 95 %
des lignes et 100 % des fonctions, puis parse `catalog.v1.json`. Il refuse avec
des codes fermés lorsque :

- le statut n'est plus `research-non-normative` ;
- la source n'a pas un SHA Git complet, une date ISO ou la licence attendue ;
- les identifiants sont dupliqués ou une source référencée n'existe pas ;
- une autorité, un état ou une disposition sort de sa liste fermée ;
- une question n'est pas formulée comme telle ou aucun scénario de panne n'est
  fourni ;
- un nom LangGraph, LangChain ou LangSmith fuit dans un identifiant de contrat
  candidat ;
- un pattern refusé ou purement interne au worker prétend créer un contrat.

Le gate ne journalise que code et chemin JSON. Il ne reflète jamais une valeur
rejetée.

## Ce que le gate ne prouve pas

La validation structurelle ne prouve ni que la description d'une source est
exacte, ni qu'un pattern est souhaitable, sûr ou complet. Ces affirmations
relèvent des revues séparées, des menaces, des vecteurs et des autorités
canoniques. Le catalogue ne fait passer aucune gate de contrat ou de runtime.

## Admission d'un nouveau pattern

Une entrée contient obligatoirement une provenance épinglée, une question
neutre, les autorités concernées, l'état Libre AI constaté, une disposition,
les éventuels contrats candidats aux noms neutres et au moins un scénario de
panne. Une entrée `refused` ou `worker-internal` ne nomme aucun contrat.

Le chemin de promotion complet est défini par ADR-0032 dans `governance` :
provenance, question, autorité, menace, sémantique indépendante, contrats,
vecteurs adversariaux, réalisation native et preuve de retrait du worker.
