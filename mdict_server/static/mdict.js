function click_sound(event) {
    // prevent default action: jump by a.href
    event.preventDefault();
    // prevent other event listener
    event.stopPropagation();
    let url = this.href;
    url = url.replace("sound://", "");
    let mdict_player = document.createElement("audio");
    mdict_player.id = 'mdict_player';
    mdict_player.src = url;
    mdict_player.play();
}

for (let element of document.getElementsByTagName('A')) {
    if (element.href) {
        let url = element.href;
        if (element.href.startsWith('sound://')) {
            element.addEventListener('click', click_sound);
        }
    }
}