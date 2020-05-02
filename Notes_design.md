Je voudrais afficher le dispositif de mesure suivant: 
 
[###########@@@oo--     |      ] 
 
 Où l'echelle est logarithmique (dBFS/LUFS), et... 

    # représente la "loudness" (je commencerai par faire un vu-metre, puis je ferai des vrais LUFS)
    @ représente le niveau crête (d'abord sample max, puis dBTP) observé au cours de la dernière frame calculée (i.e. on intègre le signal audio sur 17ms pour un écran 60Hz)
    o- représente l'historique récent du niveau crête (ex: où il était durant les dernières secondes, et il y a combien de temps environ).
    | représente le plus haut niveau crête observé, et n'est remis à zéro que sur demande.

   
Visuellement, le # pourrait être représenté en bleu clair, le @ en jaune clair, et les o- pourraient prendre la forme d'un jaune sombre qui s'estompe progressivement au fil du temps. Le | pourrait être une barre jaune clair.

Je pourrais aussi afficher une échelle sur le coté, avec un nombre de points dépendant de la taille de fenêtre, et en marquant les points recommandés par l'ITU du -1 dBTP et -23 LUFS. Ces points devraient être configurables pour les gens qui ont d'autres priorités (ex: -10dBTP quand on fait de l'enregistrement, -14LUFS quand on cible des sites comme youtube...). 

  ---------- 
 
  Au niveau du code, je verrais bien une architecture à deux threads: 

    Le thread audio, basé sur JACK, reçoit les données son et fait le gros des calculs (sauf ceux qu'il n'y a de raison d'effectuer qu'au dernier moment, comme le K-weighting des LUFS). Il met ses résultats dans des structures partagées non bloquantes.
    Le thread graphique, basé sur wgpu-rs, se reveille à chaque frame, récupère les données du thread audio, fait les calculs de dernière minute, et fait le rendu sur GPU.


Il pourrait être intéressant de découpler l'affichage des calculs audio, au cas où j'aurais envie de m'amuser avec d'autres backends comme une TUI en console, et de découpler le thread audio du système pour pouvoir supporter Pulse, ASIO, et autre. Mais ce n'est pas mon objectif premier, ça pourra être fait dans un deuxième temps. 
  
   ---------- 
  
Du côté du thread audio, on reçoit des échantillons et... 

     Pour les pics, on calcule le max, et on fait un atomic-max (émulé via du CAS ?) avec une variable atomique partagée.
    Pour les dBTP, c'est grosso modo pareil, mais en commençant par du suréchantillonage et un filtrage passe-bas qui vont demander un peu de buffering.
    Pour les VU et LU, on maintient un tampon circulaire initialement à zéro, de la longueur de fenêtre désirée (400ms + longueur de filtre ?) en échantillons, en écrasant joyeusement les anciennes données au fur et à mesure que les nouvelles arrivent.


C'est au thread graphique de faire la réduction des VU/LU, parce que dans le cas des LUFS elle implique une convolution FIR onéreuse. Pour la même raison, les données sont stockées en valeur d'échantillons absolues flottantes (de 0.0 à 1.0), on ne les convertit en dB qu'à la fin. 

Au niveau architecture, on pourrait encapsuler ça sous la forme d'un objet qui est construit pour un taux d'échantillonnage donné, qui reçoit des slices d'échantillons flottants de la part du thread audio, et qui fournit des valeurs pic/loudness à la demande (en faisant le calcul à ce moment-là), avec garantie de ne pas faire d'allocation. 

   ---------- 
  
   Du côté du thread graphique, la v1 pourrait produire un simple quad qui remplit toute la fenêtre. On aura tout le temps de faire du raffinement comme des bordures noires et des échelles plus tard, et ça ne changera vraiment pas grand-chose au niveau géométrie (juste des coordonnées de vertex). 
  
   Après, je vois deux approches: 

    Tout calculer dans le fragment shader comme un gros bourrin, en n'envoyant au GPU que les valeurs dBTP, dBTP_max, LUFS et temps, via des push constants. C'est simple, indépendant de la résolution, et ça minimise les draw calls. Mais il y a énormément de duplication d'effort puisqu'on recalcule ce qui est de facto une image 1D une fois par colonne de l'image 2D. 
    Calculer une image 1D, soit sur le GPU soit sur le CPU, puis rendre le quad en utilisant cette image comme texture. Cela évite la duplication d'effort, mais soit on fait deux draw-calls, soit on doit faire une upload de texture CPU-GPU (peut-être en mappant le buffer GPU côté CPU ?). De plus, il y a un paramètre de résolution à fixer. 


Je serais partant de faire le calcul côté GPU, et d'utiliser une résolution raisonnable, par exemple 1dB (l'oreille humaine peut vraiment entendre des différences plus faibles ?), ou éventuellement 0.1dB si je veux faire plaisir aux nazis, le tout sur une plage de dB raisonnable (de 0 à -96 ?). Je pourrais afficher cette resolution avec une interpolation nearest, qui donnera aux gros zooms un rendu "barregraphe" pas si désagréable à l'oeil et indiquant clairement quelle est la limite de résolution. 
   
   Après, c'est au backend graphique de faire sa tambouille pour afficher les rectangles. Dans mon cas, le CPU pousse la nouvelle texture, et le GPU fait un quad avec vertex passthrough et fragment qui fait un fetch du pixel correspondant depuis un sampler nearest. 

   ---------- 

   Il y a un cas intéressant quand le backend graphique est _moins_ résolu que la texture. Faut-il sous-echantilloner ou moyenner ? Les deux sont bofs. Dans le premier cas on perd des infos importantes comme le compteur de dBTP_max, et dans le second cas on floute ce qui peut obscurcir la différence dBTP actuel vs ces dernières secondes ainsi que la frontière LUFS/dBTP. 
 
   Une solution est d'introduire un feedback qui permet au backend graphique de réduire la résolution de l'histogramme-texture. 
  
   ---------- 
  
   Plus tard, je pourrai supporter plusieurs vumetres à la fois.