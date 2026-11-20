<?php
foreach (['aaguids', 'aaguid-icons'] as $name) {
 file_put_contents('/output/'.$name.'.json', json_encode(require '/upstream/resources/'.$name.'.php', JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n");
}
