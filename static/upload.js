const url = new URL(window.location.href);
let match = url.pathname.match(new RegExp("^(/.*)?/upload/?$", "i"));
console.log(match);
var serve_prefix = "";
if(match !== null && match[1] !== undefined)
{
    serve_prefix = match[1];
}

function postFile() {
    var formdata = new FormData();
    formdata.append('Desc', document.getElementById('Desc').value);
    let files_control = document.getElementById('FilesToUpload');
    let total_size = 0;
    for(let i = 0; i < files_control.files.length; i++)
    {
        formdata.append('FileToUpload', files_control.files[i]);
        total_size += files_control.files[i].size;
    }
    var request = new XMLHttpRequest();

    request.upload.addEventListener('progress', function (e) {
        if (e.loaded <= total_size) {
            var percent = Math.round(e.loaded / total_size * 100);
            document.getElementById('ProgressBar').style.width = percent + '%';
            document.getElementById('ProgressBar').innerHTML = percent + '%';
        }

        if(e.loaded == e.total){
            document.getElementById('ProgressBar').style.width = '100%';
            document.getElementById('ProgressBar').innerHTML = '100%';
        }
    });

    request.open('post', serve_prefix + '/upload/');
    request.timeout = 3600000;
    request.send(formdata);
}
